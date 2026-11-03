//! Custom Canvas, as the Android app's (ui/CustomCanvasUi.kt), over the whole window: the
//! sections (dimensions, colour profile, time-lapse settings, canvas properties) on the left, the
//! chosen one on the right under the canvas's name, Cancel and Done. Sizes are typed on a keypad of
//! its own (or the keyboard), in millimetres, centimetres, inches or pixels; the background's colour
//! is picked on the compact picker.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use procraft_core::{
    CanvasPreset, Palettes, SRGB, cmyk_profiles, color_profiles, is_cmyk_profile, layers_for, per_inch, preset_height,
    preset_number, preset_width,
};

use crate::colours::HarmonyKind;
use crate::compact_colours::{CompactColours, Picked};
use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};

const SCREEN: Themed = Themed(0xFF1F1F1F, 0xFFF2F2F2);
const PANEL: Themed = Themed(0xFF282828, 0xFFDFDFDF);
const INK: Themed = Themed(0xFFAAAAAA, 0xFF3E3E3E);
const IDLE_INK: Themed = Themed(0xFF797979, 0xFF3E3E3E);
const EDITING: u32 = 0xFF436BF0;
const KEYPAD: Themed = Themed(0xFF1A1A1A, 0xFFD1D3D8);
const DIGIT: Themed = Themed(0xFF525252, 0xFFFFFFFF);
const KEY: Themed = Themed(0xFF2B2B2B, 0xFFABB2BB);
const UNIT_CHOSEN: Themed = Themed(0xFFA1A1A1, 0xFF818C96);
const UNIT_CHOSEN_INK: Themed = Themed(0xFF1F1F1F, 0xFFFFFFFF);
const SWITCH_TRACK: Themed = Themed(0xFF353535, 0xFFD5D5D5);
const SWITCH_IDLE: Themed = Themed(0xFFC1C1C1, 0xFF3B3B3B);
const CAPSULE: Themed = Themed(0xFF1A1A1A, 0xFFFBFBFB);
const CAPSULE_EDGE: Themed = Themed(0xFF373737, 0xFFE0E0E0);
const CAPSULE_INK: u32 = 0xFFA5A5A5;
const DONE: u32 = 0xFF0190FF;
const SWATCH_EDGE: u32 = 0xFF555555;
const ROW: f32 = 47.0;
/// The page, right of the sections.
const PAGE_LEFT: f32 = 372.0;
const PAGE_TOP: f32 = 120.0;
/// Where the last row the keypad types into (DPI) ends, down from the top: two rows and their
/// line, 24 between the groups, one row.
const TYPED_BOTTOM: f32 = PAGE_TOP + ROW * 2.0 + 1.0 + 24.0 + ROW;

/// The units sizes are typed in: the key's name, the unit's (as presets keep it) and its mark.
const UNITS: [(&str, &str, &str); 4] =
    [("Millimeters", "mm", "mm"), ("Centimeters", "cm", "cm"), ("Inches", "in", "\u{2033}"), ("Pixels", "px", "px")];

const SECTIONS: [(&str, &Icon, f32); 4] = [
    ("Dimensions", &icons::INFO_DIMENSIONS, 50.0),
    ("Color profile", &icons::INFO_PROFILE, 35.0),
    ("Time-lapse settings", &icons::INFO_VIDEO, 60.0),
    ("Canvas properties", &icons::TAB_CANVAS, 40.0),
];

/// The fields the keypad types into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Width,
    Height,
    Dpi,
}

/// How Custom Canvas ended.
pub enum Done {
    Cancel,
    /// Done, as this preset.
    Made(CanvasPreset),
}

pub struct CustomCanvas {
    /// The preset edited (its place in the list); none for a new one.
    pub editing: Option<usize>,
    section: usize,
    name: String,
    /// The size in the unit chosen (its place in [`UNITS`]), and the DPI.
    width: f32,
    height: f32,
    dpi: f32,
    unit: usize,
    /// The field typed into, and what has been typed until it is left.
    field: Option<Field>,
    typed: String,
    background: u32,
    hidden: bool,
    resolution: u32,
    quality: u32,
    hevc: bool,
    profile: String,
    /// Which profiles the list shows.
    cmyk: bool,
    /// The background's colour being picked.
    picking: Option<CompactColours>,
}

impl CustomCanvas {
    /// Custom Canvas on `start` (the preset at that place in the list), or on a new canvas.
    pub fn new(start: Option<(usize, CanvasPreset)>) -> CustomCanvas {
        let (editing, p) = match start {
            Some((i, p)) => (Some(i), p),
            None => (
                None,
                CanvasPreset {
                    name: "Untitled Canvas".into(),
                    unit: "px".into(),
                    size_width: 1080.0,
                    size_height: 1080.0,
                    dpi: 300.0,
                    background: 0xFF_FFFF,
                    background_hidden: false,
                    time_lapse_resolution: 0,
                    time_lapse_quality: 1,
                    time_lapse_hevc: false,
                    profile: SRGB.into(),
                },
            ),
        };
        CustomCanvas {
            editing,
            section: 0,
            name: p.name.clone(),
            width: p.size_width,
            height: p.size_height,
            dpi: p.dpi,
            unit: UNITS.iter().position(|u| u.1 == p.unit).unwrap_or(3),
            field: Some(Field::Width),
            typed: String::new(),
            background: p.background,
            hidden: p.background_hidden,
            resolution: p.time_lapse_resolution,
            quality: p.time_lapse_quality,
            hevc: p.time_lapse_hevc,
            cmyk: is_cmyk_profile(p.profile.clone()),
            profile: p.profile,
            picking: None,
        }
    }

    fn preset(&self) -> CanvasPreset {
        let name = if self.name.trim().is_empty() { "Untitled Canvas".to_owned() } else { self.name.trim().to_owned() };
        CanvasPreset {
            name,
            unit: UNITS.get(self.unit).map_or("px", |u| u.1).to_owned(),
            size_width: self.width,
            size_height: self.height,
            dpi: self.dpi,
            background: self.background,
            background_hidden: self.hidden,
            time_lapse_resolution: self.resolution,
            time_lapse_quality: self.quality,
            time_lapse_hevc: self.hevc,
            profile: self.profile.clone(),
        }
    }

    /// What typing has made of the field so far goes into it.
    fn commit(&mut self) {
        if let Ok(v) = self.typed.parse::<f32>()
            && v > 0.0
        {
            match self.field {
                Some(Field::Width) => self.width = v,
                Some(Field::Height) => self.height = v,
                Some(Field::Dpi) => self.dpi = v.clamp(1.0, 9999.0),
                None => {}
            }
        }
        self.typed.clear();
    }

    /// The size kept, in another unit: straight from one length to another, through the pixels
    /// otherwise.
    fn convert(&mut self, to: usize) {
        let p = self.preset();
        let (px_w, px_h) = (preset_width(p.clone()) as f32, preset_height(p) as f32);
        let from = UNITS.get(self.unit).and_then(|u| per_inch(u.1.into()));
        let into = UNITS.get(to).and_then(|u| per_inch(u.1.into()));
        let change = |v: f32, px: f32| match (into, from) {
            (Some(k), Some(f)) => v / f * k,
            (Some(k), None) => px / self.dpi * k,
            (None, _) => px,
        };
        (self.width, self.height) = (change(self.width, px_w), change(self.height, px_h));
        self.unit = to;
    }

    fn key(&mut self, k: char) {
        match k {
            '\u{8}' => _ = self.typed.pop(),
            '.' => {
                if !self.typed.contains('.') && self.field != Some(Field::Dpi) && self.unit != 3 {
                    self.typed.push('.');
                }
            }
            d if d.is_ascii_digit() && self.typed.len() < 7 => self.typed.push(d),
            _ => {}
        }
    }

    fn next(&mut self) {
        self.commit();
        self.field = Some(match self.field {
            Some(Field::Width) => Field::Height,
            Some(Field::Height) => Field::Dpi,
            _ => Field::Width,
        });
    }

    /// Shows it over the whole window; returns how it ended, once it has.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        palettes: &Palettes,
        harmony: HarmonyKind,
    ) -> Option<Done> {
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let mut done = None;
        let mut name_focused = false;
        egui::Area::new(Id::unique("custom canvas")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0, argb(SCREEN));
            let ink = argb(INK);
            // the sections
            let rail =
                Rect::from_min_max(screen.min + vec2(10.0, 10.0), pos2(screen.left() + 329.0, screen.bottom() - 10.0));
            painter.rect_filled(rail, 20.0, argb(PANEL));
            text(&painter, rail.min + vec2(24.0, 16.0), Align2::LEFT_TOP, "Custom Canvas", 22.0, Weight::Bold, ink);
            for (i, (label, icon, size)) in SECTIONS.into_iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(rail.left() + 8.0, rail.top() + 59.0 + 62.0 * i as f32),
                    vec2(304.0, 52.0),
                );
                let chosen = i == self.section;
                if chosen {
                    painter.rect_filled(row, 26.0, argb(pc::ACCENT));
                }
                let colour = if chosen { Color32::WHITE } else { argb(IDLE_INK) };
                // icons centred 43 in, whatever their size; the names 69 in
                look.icon(ui, icon, pos2(rail.left() + 43.0, row.center().y), size, colour);
                text(
                    &painter,
                    pos2(rail.left() + 69.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    15.0,
                    Weight::Regular,
                    colour,
                );
                if ui.interact(row, ui.scope_id().with(("section", i)), Sense::click()).clicked() {
                    self.commit();
                    self.section = i;
                }
            }
            // the name, centred over the page (upright, from its left)
            let field = if wide {
                Rect::from_min_max(
                    pos2(screen.left() + PAGE_LEFT, screen.top() + 28.0),
                    pos2(screen.right() - 32.0, screen.top() + 56.0),
                )
            } else {
                Rect::from_min_max(
                    pos2(screen.left() + 394.0, screen.top() + 28.0),
                    pos2(screen.right() - 160.0, screen.top() + 56.0),
                )
            };
            let edit = egui::TextEdit::singleline(&mut self.name)
                .font(Weight::Bold.font(22.0))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .align(if wide { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER })
                .desired_width(field.width())
                .char_limit(64)
                .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
            name_focused = edit.response.has_focus();
            // Cancel, and Import beside it on the colour profile page, then Done
            let tick = Rect::from_min_size(pos2(screen.right() - 20.0 - 44.0, screen.top() + 20.0), vec2(44.0, 45.0));
            let words: &[&str] = if self.section == 1 { &["Cancel", "Import"] } else { &["Cancel"] };
            match capsule_words(ui, tick.left() - 13.0, screen.top() + 21.0, words, CAPSULE_INK.into()) {
                Some("Cancel") => done = Some(Done::Cancel),
                Some(_) => {
                    if let Some(name) = crate::files::import_profile() {
                        self.cmyk = is_cmyk_profile(name.clone());
                        self.profile = name;
                    }
                }
                None => {}
            }
            painter.rect_filled(tick, 22.0, argb(DONE));
            look.icon(ui, &icons::DONE_TICK, tick.center(), 40.0, Color32::WHITE);
            if ui.interact(tick, ui.scope_id().with("done"), Sense::click()).clicked() {
                self.commit();
                done = Some(Done::Made(self.preset()));
            }
            let page = Rect::from_min_max(
                pos2(screen.left() + PAGE_LEFT, screen.top() + PAGE_TOP),
                pos2(screen.right() - 32.0, screen.bottom()),
            );
            match self.section {
                0 => self.dimensions(ui, page),
                1 => self.profiles(ui, look, page),
                2 => self.time_lapse(ui, look, page),
                _ => self.properties(ui, page),
            }
            if self.section == 0 && self.field.is_some() {
                self.keypad(ui, screen, wide);
            }
        });
        // the keyboard types sizes too, while the name isn't being typed
        if self.section == 0 && self.field.is_some() && !name_focused {
            let mut keys = Vec::new();
            ctx.input(|i| {
                for e in &i.events {
                    match e {
                        egui::Event::Text(t) => keys.extend(t.chars().filter(|c| c.is_ascii_digit() || *c == '.')),
                        egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => keys.push('\u{8}'),
                        egui::Event::Key { key: egui::Key::Tab, pressed: true, .. } => keys.push('\t'),
                        egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => keys.push('\n'),
                        _ => {}
                    }
                }
            });
            for k in keys {
                match k {
                    '\t' => self.next(),
                    '\n' => {
                        self.commit();
                        self.field = None;
                    }
                    k => self.key(k),
                }
            }
        }
        // the background's colour, in the compact picker hanging from its swatch's row
        if let Some(pick) = self.picking.as_mut() {
            let size = pick.size();
            let rect = Rect::from_min_size(pos2(screen.right() - size.x, screen.top() + 168.0), size);
            let p3 = procraft_core::colours_in_p3(self.profile.clone());
            match pick.show(ctx, look, rect, rect.left() + 200.0, palettes, harmony, p3) {
                Some(Picked::Colour(rgb)) => self.background = rgb,
                Some(Picked::Closed) => self.picking = None,
                None => {}
            }
        }
        done
    }

    /// Width and height, then DPI and the most layers a canvas that size holds.
    fn dimensions(&mut self, ui: &egui::Ui, page: Rect) {
        let mark = UNITS.get(self.unit).map_or("px", |u| u.2);
        let shown = |me: &Self, f: Field, v: String| {
            if me.field == Some(f) && !me.typed.is_empty() { me.typed.clone() } else { v }
        };
        let first = group(ui, page.left_top(), page.width(), 2);
        let rows = [
            ("Width", shown(self, Field::Width, preset_number(self.width)), mark, Some(Field::Width)),
            ("Height", shown(self, Field::Height, preset_number(self.height)), mark, Some(Field::Height)),
        ];
        let second_top = first.bottom() + 24.0;
        let p = self.preset();
        let layers = layers_for(preset_width(p.clone()), preset_height(p)).to_string();
        let more = [
            ("DPI", shown(self, Field::Dpi, format!("{}", self.dpi.round() as i64)), "", Some(Field::Dpi)),
            ("Maximum Layers", layers, "", None),
        ];
        group(ui, pos2(page.left(), second_top), page.width(), 2);
        for (k, (label, value, unit, f)) in rows.into_iter().chain(more).enumerate() {
            let top = if k < 2 { first.top() } else { second_top } + (ROW + 1.0) * (k % 2) as f32;
            let row = Rect::from_min_size(pos2(page.left(), top), vec2(page.width(), ROW));
            let ink = argb(INK);
            text(
                ui.painter(),
                pos2(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                15.0,
                Weight::Regular,
                ink,
            );
            let editing = f.is_some() && self.field == f;
            let unit_shown = if unit.is_empty() { String::new() } else { format!(" {unit}") };
            let at = pos2(row.right() - 16.0, row.center().y);
            let after = text(ui.painter(), at, Align2::RIGHT_CENTER, &unit_shown, 15.0, Weight::Regular, ink);
            let colour = if editing { argb(EDITING) } else { ink };
            text(
                ui.painter(),
                pos2(after.left(), row.center().y),
                Align2::RIGHT_CENTER,
                &value,
                15.0,
                Weight::Regular,
                colour,
            );
            if let Some(f) = f
                && ui.interact(row, ui.scope_id().with(("size row", k)), Sense::click()).clicked()
            {
                self.commit();
                self.field = Some(f);
            }
        }
    }

    /// RGB or CMYK, then the profiles of that kind, the chosen one blue.
    fn profiles(&mut self, ui: &egui::Ui, look: &mut Look, page: Rect) {
        let switch = Rect::from_min_size(page.left_top(), vec2(page.width(), 34.0));
        ui.painter().rect_filled(switch, 17.0, argb(SWITCH_TRACK));
        for (i, (label, icon)) in [("RGB", &icons::PROFILE_RGB), ("CMYK", &icons::PROFILE_CMYK)].into_iter().enumerate()
        {
            let half = Rect::from_min_size(
                switch.min + vec2(2.0 + (switch.width() - 4.0) / 2.0 * i as f32, 2.0),
                vec2((switch.width() - 4.0) / 2.0, 30.0),
            );
            let on = self.cmyk == (i == 1);
            if on {
                ui.painter().rect_filled(half, 15.0, argb(pc::ACCENT));
            }
            let ink = if on { Color32::WHITE } else { argb(SWITCH_IDLE) };
            let label_width = 15.0 * 0.6 * label.len() as f32;
            let start = half.center().x - (20.0 + 8.0 + label_width) / 2.0;
            look.icon(ui, icon, pos2(start + 10.0, half.center().y), 20.0, ink);
            text(
                ui.painter(),
                pos2(start + 28.0, half.center().y),
                Align2::LEFT_CENTER,
                label,
                15.0,
                Weight::Regular,
                ink,
            );
            if ui.interact(half, ui.scope_id().with(("kind", i)), Sense::click()).clicked() {
                self.cmyk = i == 1;
            }
        }
        let names = if self.cmyk { cmyk_profiles() } else { color_profiles() };
        let top = switch.bottom() + 24.0;
        group(ui, pos2(page.left(), top), page.width(), names.len());
        for (i, name) in names.iter().enumerate() {
            let row = Rect::from_min_size(pos2(page.left(), top + (ROW + 1.0) * i as f32), vec2(page.width(), ROW));
            if choice_row(
                ui,
                row,
                name,
                *name == self.profile,
                None,
                false,
                ("profile", i),
                look,
                i == 0,
                i + 1 == names.len(),
            ) {
                self.profile = name.clone();
            }
        }
    }

    /// The video's resolution, its quality, and HEVC.
    fn time_lapse(&mut self, ui: &egui::Ui, look: &mut Look, page: Rect) {
        let sizes = [("1080p", "1920 \u{d7} 1080px"), ("2K", "2732 \u{d7} 2048px"), ("4K", "3840 \u{d7} 2160px")];
        group(ui, page.left_top(), page.width(), 3);
        for (i, (label, detail)) in sizes.into_iter().enumerate() {
            let row =
                Rect::from_min_size(pos2(page.left(), page.top() + (ROW + 1.0) * i as f32), vec2(page.width(), ROW));
            if choice_row(
                ui,
                row,
                label,
                i as u32 == self.resolution,
                Some(detail),
                false,
                ("resolution", i),
                look,
                i == 0,
                i == 2,
            ) {
                self.resolution = i as u32;
            }
        }
        let top = page.top() + (ROW + 1.0) * 3.0 - 1.0 + 24.0;
        let qualities = ["Low quality", "Good quality", "Studio quality", "Lossless"];
        group(ui, pos2(page.left(), top), page.width(), 4);
        for (i, label) in qualities.into_iter().enumerate() {
            let row = Rect::from_min_size(pos2(page.left(), top + (ROW + 1.0) * i as f32), vec2(page.width(), ROW));
            if choice_row(ui, row, label, i as u32 == self.quality, None, true, ("quality", i), look, i == 0, i == 3) {
                self.quality = i as u32;
            }
        }
        let top = top + (ROW + 1.0) * 4.0 - 1.0 + 24.0;
        group(ui, pos2(page.left(), top), page.width(), 1);
        let row = Rect::from_min_size(pos2(page.left(), top), vec2(page.width(), ROW));
        if switch_line(ui, row, "HEVC", self.hevc, "hevc") {
            self.hevc = !self.hevc;
        }
    }

    /// The background's colour (a tap opens the compact picker), and whether it is hidden.
    fn properties(&mut self, ui: &egui::Ui, page: Rect) {
        group(ui, page.left_top(), page.width(), 1);
        let row = Rect::from_min_size(page.left_top(), vec2(page.width(), ROW));
        let ink = argb(INK);
        text(
            ui.painter(),
            pos2(row.left() + 16.0, row.center().y),
            Align2::LEFT_CENTER,
            "Background color",
            15.0,
            Weight::Regular,
            ink,
        );
        let swatch = pos2(row.right() - 16.0 - 12.0, row.center().y);
        let rgb = self.background;
        ui.painter().circle_filled(swatch, 12.0, Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
        ui.painter().circle_stroke(swatch, 11.5, Stroke::new(1.0, argb(SWATCH_EDGE)));
        if ui.interact(row, ui.scope_id().with("background"), Sense::click()).clicked() {
            let mut pick = CompactColours::new(self.background, 0.5);
            pick.arrow_below = false;
            self.picking = Some(pick);
        }
        let top = row.bottom() + 24.0;
        group(ui, pos2(page.left(), top), page.width(), 1);
        let row = Rect::from_min_size(pos2(page.left(), top), vec2(page.width(), ROW));
        if switch_line(ui, row, "Background hidden", self.hidden, "hidden") {
            self.hidden = !self.hidden;
        }
    }

    /// The keypad: units on the left, the digits, and Backspace, Next and Done on the right. On a
    /// wide window its keys are 150 x 87.5 (9 apart down) in a 457 panel, the rows from 47 below
    /// its top; upright 120 x 71 (8 apart) in a 342 one, centred in it. In a window too short for
    /// that, it and its keys are that much shorter (to half), so it stays under the rows it types
    /// into.
    fn keypad(&mut self, ui: &egui::Ui, screen: Rect, wide: bool) {
        let full = if wide { 457.0 } else { 342.0 };
        let fit = ((screen.height() - TYPED_BOTTOM) / full).clamp(0.5, 1.0);
        let (w, h, down, across) = if wide { (150.0, 87.5, 9.0, 9.0) } else { (120.0, 71.0, 8.0, 9.0) };
        let (h, down) = (h * fit, down * fit);
        let panel = Rect::from_min_max(pos2(screen.left(), screen.bottom() - full * fit), screen.max);
        ui.painter().rect_filled(panel, 0, argb(KEYPAD));
        // the panel takes its own presses
        ui.interact(panel, ui.scope_id().with("keypad"), Sense::click());
        let content = vec2(w + 30.0 + w * 3.0 + across * 2.0 + 30.0 + w, h * 4.0 + down * 3.0);
        let left = panel.center().x - content.x / 2.0;
        let top = if wide { panel.top() + 47.0 * fit } else { panel.center().y - content.y / 2.0 };
        let key = |label: &str, rect: Rect, fill: Themed, ink: Color32, size: f32, id: (&str, usize)| {
            ui.painter().rect_filled(rect, 8.0, argb(fill));
            text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, size, Weight::Regular, ink);
            ui.interact(rect, ui.scope_id().with(id), Sense::click()).clicked()
        };
        let ink = argb(INK);
        for (i, (label, _, _)) in UNITS.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(left, top + (h + down) * i as f32), vec2(w, h));
            let chosen = i == self.unit;
            let (fill, colour) = if chosen { (UNIT_CHOSEN, argb(UNIT_CHOSEN_INK)) } else { (KEY, ink) };
            if key(label, rect, fill, colour, 15.0, ("unit", i)) && !chosen {
                self.commit();
                self.convert(i);
            }
        }
        let digits_left = left + w + 30.0;
        for (r, row) in ["789", "456", "123"].into_iter().enumerate() {
            for (c, d) in row.chars().enumerate() {
                let rect = Rect::from_min_size(
                    pos2(digits_left + (w + across) * c as f32, top + (h + down) * r as f32),
                    vec2(w, h),
                );
                if key(&d.to_string(), rect, DIGIT, ink, 26.0, ("digit", r * 3 + c)) {
                    self.key(d);
                }
            }
        }
        let last = top + (h + down) * 3.0;
        if key(
            "0",
            Rect::from_min_size(pos2(digits_left, last), vec2(w * 2.0 + across, h)),
            DIGIT,
            ink,
            26.0,
            ("digit", 10),
        ) {
            self.key('0');
        }
        if key(
            ".",
            Rect::from_min_size(pos2(digits_left + (w + across) * 2.0, last), vec2(w, h)),
            DIGIT,
            ink,
            26.0,
            ("digit", 11),
        ) {
            self.key('.');
        }
        let right = digits_left + w * 3.0 + across * 2.0 + 30.0;
        if key("Backspace", Rect::from_min_size(pos2(right, top), vec2(w, h)), KEY, ink, 15.0, ("side", 0)) {
            self.key('\u{8}');
        }
        if key("Next", Rect::from_min_size(pos2(right, top + h + down), vec2(w, h)), KEY, ink, 15.0, ("side", 1)) {
            self.next();
        }
        let done = Rect::from_min_size(pos2(right, top + (h + down) * 2.0), vec2(w, h * 2.0 + down));
        if key("Done", done, KEY, ink, 15.0, ("side", 2)) {
            self.commit();
            self.field = None;
        }
    }
}

/// A header's capsule of words, ending at `right`: each a button. Returns the one tapped.
pub(crate) fn capsule_words<'a>(
    ui: &egui::Ui,
    right: f32,
    top: f32,
    words: &[&'a str],
    ink: Themed,
) -> Option<&'a str> {
    let galleys: Vec<_> = words
        .iter()
        .map(|w| ui.painter().layout_no_wrap((*w).to_owned(), Weight::Regular.font(17.0), argb(ink)))
        .collect();
    let width: f32 = galleys.iter().map(|g| g.size().x + 26.0).sum();
    let capsule = Rect::from_min_size(pos2(right - width, top), vec2(width, 43.0));
    ui.painter().rect_filled(capsule, 21.5, argb(CAPSULE));
    ui.painter().rect_stroke(capsule, 21.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
    let mut x = capsule.left();
    let mut tapped = None;
    for (word, galley) in words.iter().zip(galleys) {
        let button = Rect::from_min_size(pos2(x, top), vec2(galley.size().x + 26.0, 43.0));
        let at = button.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley.clone(), argb(ink));
        if ui.interact(button, ui.scope_id().with(("capsule", *word)), Sense::click()).clicked() {
            tapped = Some(*word);
        }
        x = button.right();
    }
    tapped
}

/// A group of `rows` rows from `at`, `width` wide (rounded 16), a line between each; returns it.
fn group(ui: &egui::Ui, at: Pos2, width: f32, rows: usize) -> Rect {
    let rect = Rect::from_min_size(at, vec2(width, (ROW + 1.0) * rows as f32 - 1.0));
    ui.painter().rect_filled(rect, 16.0, argb(PANEL));
    for k in 1..rows {
        let line = Rect::from_min_size(pos2(rect.left(), rect.top() + (ROW + 1.0) * k as f32 - 1.0), vec2(width, 1.0));
        ui.painter().rect_filled(line, 0, argb(SCREEN));
    }
    rect
}

/// A choice in a group: blue when chosen (rounded with the group at its ends), a detail at its
/// right or, chosen, a tick. Returns whether it was tapped.
#[allow(clippy::too_many_arguments)]
fn choice_row(
    ui: &egui::Ui,
    row: Rect,
    label: &str,
    chosen: bool,
    detail: Option<&str>,
    tick: bool,
    id: (&str, usize),
    look: &mut Look,
    first: bool,
    last: bool,
) -> bool {
    if chosen {
        let corners = egui::CornerRadius {
            nw: if first { 16 } else { 0 },
            ne: if first { 16 } else { 0 },
            sw: if last { 16 } else { 0 },
            se: if last { 16 } else { 0 },
        };
        ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
    }
    let ink = if chosen { Color32::WHITE } else { argb(INK) };
    text(ui.painter(), pos2(row.left() + 16.0, row.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
    if let Some(d) = detail {
        text(
            ui.painter(),
            pos2(row.right() - 16.0, row.center().y),
            Align2::RIGHT_CENTER,
            d,
            15.0,
            Weight::Regular,
            ink,
        );
    }
    if tick && chosen {
        look.icon(ui, &icons::MENU_TICK, pos2(row.right() - 16.0 - 10.0, row.center().y), 20.0, Color32::WHITE);
    }
    ui.interact(row, ui.scope_id().with(id), Sense::click()).clicked()
}

/// A switch's row: its name, and the switch at its right. Returns whether it was tapped.
fn switch_line(ui: &egui::Ui, row: Rect, label: &str, on: bool, id: &str) -> bool {
    text(
        ui.painter(),
        pos2(row.left() + 16.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        15.0,
        Weight::Regular,
        argb(INK),
    );
    crate::transform::switch(ui, pos2(row.right() - 8.0 - 63.0, row.center().y - 14.0), on, id);
    ui.interact(row, ui.scope_id().with(id), Sense::click()).clicked()
}
