//! The brush studio's source editor, as the Android app's (ui/SourceEditorUi.kt): the shape, the
//! grain, or what the brush paints metalness or roughness through, in a 600 square. Import takes a
//! photo, a file, a shipped image from the source library or a picture on the clipboard, and a
//! two-finger tap turns the image negative; all but the shape can also be made to repeat without
//! seams. Cancel leaves the brush as it was, the tick keeps the change.

use std::collections::HashSet;

use egui::epaint::RectShape;
use egui::{
    Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, UiBuilder, pos2, vec2,
};
use procraft_core::{CanvasSession, SourceKind};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel;

const PAGE: Themed = Themed(0xFF2A2A2A, 0xFFE5E5E5);
const VIEW: Themed = Themed(0xFF262626, 0xFFE5E5E5);
const TITLE: Themed = Themed(0xFFEEEEEE, 0xFF333333);
const CAPSULE: Themed = Themed(0xFF1C1C1C, 0xFFFFFFFF);
const CAPSULE_EDGE: Themed = Themed(0xFF383838, 0xFFE0E0E0);
const CAPSULE_INK: Themed = Themed(0xFFAAAAAA, 0xFF585858);
const DONE: u32 = 0xFF0190FF;
const REPEAT_LABEL: u32 = 0xFF808080;
const REPEAT_TRACK: u32 = 0xFF3E3E3E;
const LIGHT_PANEL: u32 = 0xFFD5D5D5;
const LIGHT_TEXT: u32 = 0xFF393939;
const LIGHT_ROW: u32 = 0xFFDEDEDE;
const LIGHT_SEPARATOR: u32 = 0xFFD2D2D2;
const MENU_BLUE: u32 = 0xFF0A84FF;
const LIBRARY_VEIL: u32 = 0x80000000;
const LIBRARY_SHEET: Themed = Themed(0xFF1C1C1E, 0xFFF2F2F7);
const LIBRARY_TITLE: Themed = Themed(0xFFC1C1C1, 0xFF3A3A3A);
const LIBRARY_DIM: u32 = 0xFF868686;
const LIBRARY_EDGE: Themed = Themed(0xFF5A5A5A, 0xFFC8C8C8);
const SEARCH_FILL: Themed = Themed(0xFF1B1B1B, 0xFFE6E6EA);
const SEARCH_EDGE: Themed = Themed(0xFF363636, 0xFFD8D8DC);
const SEARCH_HINT: Themed = Themed(0xFF787878, 0xFF8A8A8E);
/// How far from the window's left a wide window's editor starts, the pages' card showing beside it.
const WIDE_LEFT: f32 = 246.0;
/// The source library's pictures and the gaps between them.
const CELL: f32 = 147.0;
const CELL_GAP: f32 = 53.0;
const CELL_HEIGHT: f32 = 146.0 + 5.0 + 21.0;
const CELL_ROW_GAP: f32 = 18.0;
const PICTURES: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];

/// Where Import's pictures come from, in its menu's order.
const IMPORTS: [(&Icon, &str, f32); 4] = [
    (&icons::SOURCE_PHOTO_FILL, "Import a photo", 30.0),
    (&icons::FOLDER_FILL, "Import a file", 30.0),
    (&icons::SOURCE_BOOKS_FILL, "Source Library", 30.0),
    (&icons::TEXT_PASTE, "Paste", 40.0),
];

/// The source library while it shows: the grains (else the shapes), what is searched for, and
/// the shipped images as pictures with their names.
struct Library {
    grains: bool,
    query: String,
    shapes: Vec<(String, TextureHandle)>,
    grain_images: Vec<(String, TextureHandle)>,
}

pub struct SourceEditor {
    kind: SourceKind,
    /// The image as edited here (empty: the round tip, or white all over), and the shipped image
    /// it is, if it is one.
    image: Vec<u8>,
    shipped: Option<u32>,
    /// Import's menu (the frame it opened in), and the source library, while they show.
    importing: Option<u64>,
    library: Option<Library>,
    /// Auto repeat, and its settings.
    repeating: bool,
    scale: f32,
    rotate: f32,
    overlap: f32,
    hardness: f32,
    mirror: bool,
    pyramid: bool,
    /// What shows, made again when something changes.
    shown: Option<TextureHandle>,
    stale: bool,
    /// The fingers on the image, and the most there were at once.
    touches: HashSet<u64>,
    most: usize,
}

impl SourceEditor {
    /// The editor for the studio brush's `kind` of source, on the image it uses now.
    pub fn new(kind: SourceKind, session: &CanvasSession) -> SourceEditor {
        SourceEditor {
            kind,
            image: session.studio_source(kind),
            shipped: None,
            importing: None,
            library: None,
            repeating: false,
            scale: 0.0,
            rotate: 0.0,
            overlap: 1.0,
            hardness: 0.0,
            mirror: false,
            pyramid: true,
            shown: None,
            stale: true,
            touches: HashSet::new(),
            most: 0,
        }
    }

    /// Whether it draws from the grains: all but the shape.
    fn grain(&self) -> bool {
        self.kind != SourceKind::Shape
    }

    /// The image as it would be kept: made to repeat while Auto repeat is on.
    fn result(&self) -> Vec<u8> {
        if self.repeating && !self.image.is_empty() {
            procraft_core::repeatable_source(
                self.image.clone(),
                1.0 + self.scale * 3.0,
                self.rotate * 360.0,
                self.overlap,
                self.hardness,
                self.mirror,
                self.pyramid,
            )
        } else {
            self.image.clone()
        }
    }

    /// A new image to edit (from a picture or the clipboard): no longer a shipped one.
    fn take(&mut self, pixels: Option<Vec<u8>>) {
        if let Some(pixels) = pixels.filter(|p| !p.is_empty()) {
            self.image = pixels;
            self.shipped = None;
            self.stale = true;
        }
    }

    /// The tick: the brush's source becomes the image (or the shipped one it is). True when the
    /// brush changed.
    fn keep(&self, session: &CanvasSession) -> bool {
        match self.shipped.filter(|_| !self.repeating) {
            Some(index) => session.set_studio_shipped_source(self.kind, index),
            None => {
                let pixels = self.result();
                !pixels.is_empty() && session.set_studio_source(self.kind, pixels)
            }
        }
    }

    /// Shows the editor over the studio (`wide`: beside the pages' card). Some once it closes:
    /// whether the brush changed.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, wide: bool) -> Option<bool> {
        if self.stale {
            self.stale = false;
            self.shown = picture(ctx, &self.result(), self.kind == SourceKind::Shape, self.repeating);
        }
        // Escape closes what is open over the editor, else the editor (as Cancel)
        let escape = crate::ui::escape(ctx) && ctx.memory(|m| m.focused().is_none());
        if escape && self.importing.take().is_none() && self.library.take().is_none() {
            return Some(false);
        }
        let screen = ctx.content_rect();
        let mut closed = None;
        egui::Area::new(Id::unique("source editor")).fixed_pos(screen.min).order(Order::Tooltip).show(ctx, |ui| {
            // nothing under it is touched while it shows
            ui.allocate_rect(screen, Sense::click_and_drag());
            let page = if wide {
                Rect::from_min_max(pos2(screen.left() + WIDE_LEFT, screen.top()), screen.max)
            } else {
                screen
            };
            let corner = if wide { CornerRadius { nw: 20, sw: 20, ne: 0, se: 0 } } else { CornerRadius::ZERO };
            if self.repeating
                && let Some(shown) = &self.shown
            {
                // the grain repeating over the whole page, at its own pixels
                let tile = shown.size()[0] as f32 / ctx.pixels_per_point();
                let uv = Rect::from_min_size(Pos2::ZERO, page.size() / tile);
                ui.painter().add(RectShape::filled(page, corner, Color32::WHITE).with_texture(shown.id(), uv));
            } else {
                ui.painter().rect_filled(page, corner, argb(PAGE));
                self.view(ui, page);
            }
            let title = match self.kind {
                SourceKind::Shape => "Shape Editor",
                SourceKind::Grain => "Grain Editor",
                SourceKind::Metallic => "Metallic Editor",
                SourceKind::Roughness => "Roughness Editor",
            };
            let at = page.min + vec2(24.0, if wide { 16.0 } else { 26.0 });
            text(ui.painter(), at, Align2::LEFT_TOP, title, 22.0, Weight::Bold, argb(TITLE));
            let top = page.top() + if wide { 10.0 } else { 20.0 };
            let right = page.right() - if wide { 12.0 } else { 20.0 };
            let done = Rect::from_min_size(pos2(right - 44.0, top), vec2(44.0, 45.0));
            let mut buttons = vec![("Import", 71.0)];
            if self.grain() {
                buttons.push(("Auto repeat", 107.0));
            }
            buttons.push(("Cancel", 73.0));
            let width: f32 = buttons.iter().map(|b| b.1).sum();
            let capsule = Rect::from_min_size(pos2(done.left() - 13.0 - width, top + 1.0), vec2(width, 43.0));
            ui.painter().rect_filled(capsule, 21.5, argb(CAPSULE));
            ui.painter().rect_stroke(capsule, 21.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
            let import = Rect::from_min_size(capsule.min, vec2(71.0, 43.0));
            let mut x = capsule.left();
            for (label, w) in buttons {
                let button = Rect::from_min_size(pos2(x, capsule.top()), vec2(w, 43.0));
                x += w;
                let on = label == "Auto repeat" && self.repeating;
                if on {
                    ui.painter().rect_filled(button, 21.5, argb(pc::ACCENT));
                }
                let ink = if on { Color32::WHITE } else { argb(CAPSULE_INK) };
                text(ui.painter(), button.center(), Align2::CENTER_CENTER, label, 17.0, Weight::Regular, ink);
                if !ui.interact(button, ui.scope_id().with(("capsule", label)), Sense::click()).clicked() {
                    continue;
                }
                match label {
                    "Import" => {
                        self.importing = match self.importing {
                            Some(_) => None,
                            None => Some(ctx.cumulative_frame_nr()),
                        };
                    }
                    "Auto repeat" => {
                        self.repeating = !self.repeating;
                        self.stale = true;
                    }
                    _ => closed = Some(false),
                }
            }
            ui.painter().rect_filled(done, 22, argb(DONE));
            look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
            if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
                closed = Some(self.keep(session));
            }
            if self.repeating {
                self.repeat_panel(ui, page);
            }
            if self.importing.is_some() {
                self.import_menu(ui, look, import);
            }
            if self.library.is_some() {
                self.library_view(ui, screen);
            }
        });
        closed
    }

    /// The image in its 600 square (the round tip a white disc, a grain of none white all over);
    /// a two-finger tap on it turns it negative.
    fn view(&mut self, ui: &Ui, page: Rect) {
        let view = Rect::from_center_size(page.center() + vec2(0.0, 32.0), vec2(600.0, 601.0));
        ui.painter().rect_filled(view, 20, argb(VIEW));
        match &self.shown {
            Some(shown) => {
                let square = Rect::from_center_size(view.center(), vec2(600.0, 600.0));
                let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                ui.painter().add(RectShape::filled(square, 20, Color32::WHITE).with_texture(shown.id(), uv));
            }
            None if self.grain() => _ = ui.painter().rect_filled(view, 20, Color32::WHITE),
            None => _ = ui.painter().circle_filled(view.center(), 240.0, Color32::WHITE),
        }
        if self.importing.is_some() || self.library.is_some() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let egui::Event::Touch { id, phase, pos, .. } = event else { continue };
            match phase {
                egui::TouchPhase::Start if view.contains(pos) => {
                    self.touches.insert(id.0);
                    self.most = self.most.max(self.touches.len());
                }
                // the last finger up: two at once were a tap
                egui::TouchPhase::End | egui::TouchPhase::Cancel
                    if self.touches.remove(&id.0) && self.touches.is_empty() =>
                {
                    if self.most == 2 && !self.image.is_empty() {
                        self.image.iter_mut().for_each(|v| *v = 255 - *v);
                        self.shipped = None;
                        self.stale = true;
                    }
                    self.most = 0;
                }
                _ => {}
            }
        }
    }

    /// Auto repeat's settings: two columns of sliders and switches in a panel along the bottom.
    fn repeat_panel(&mut self, ui: &Ui, page: Rect) {
        let panel =
            Rect::from_min_size(pos2(page.center().x - 325.0, page.bottom() - 30.0 - 228.0), vec2(650.0, 228.0));
        ui.painter().rect_filled(panel, 16, argb(PAGE));
        let inner = panel.shrink(16.0);
        let cell = |column: usize, top: f32, height: f32| {
            Rect::from_min_size(pos2(inner.left() + column as f32 * (301.0 + 16.0), top), vec2(301.0, height))
        };
        let (first, second, third) = (inner.top(), inner.top() + 55.0 + 16.0, inner.top() + 2.0 * (55.0 + 16.0));
        let one = |v: f32| {
            let s = format!("{v:.1}");
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let named = |v: f32, none: &str, all: &str| {
            if v <= 0.0 {
                none.to_owned()
            } else if v >= 1.0 {
                all.to_owned()
            } else {
                format!("{}%", (v * 100.0).round() as i32)
            }
        };
        let mut changed = false;
        let sliders = [
            (cell(0, first, 55.0), "Grain scale", one(1.0 + self.scale * 3.0), &mut self.scale),
            (cell(1, first, 55.0), "Rotate", format!("{}°", (self.rotate * 360.0).round() as i32), &mut self.rotate),
            (cell(0, second, 55.0), "Border overlap", named(self.overlap, "None", "Maximal"), &mut self.overlap),
            (cell(1, second, 55.0), "Mask hardness", named(self.hardness, "Soft", "Hard"), &mut self.hardness),
        ];
        for (rect, label, value, at) in sliders {
            if let Some(v) = repeat_slider(ui, rect, label, &value, *at) {
                changed |= *at != v;
                *at = v;
            }
        }
        for (rect, label, on) in [
            (cell(0, third, 40.0), "Mirror overlap", &mut self.mirror),
            (cell(1, third, 40.0), "Pyramid blending", &mut self.pyramid),
        ] {
            let at = pos2(rect.left() + 16.0, rect.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, label, 15.0, Weight::Regular, argb(REPEAT_LABEL));
            crate::transform::switch(ui, pos2(rect.right() - 8.0 - 63.0, rect.center().y - 14.0), *on, label);
            if ui.interact(rect, ui.scope_id().with(("repeat switch", label)), Sense::click()).clicked() {
                *on = !*on;
                changed = true;
            }
        }
        self.stale |= changed;
    }

    /// Where the image can come from, in a light popover under Import; a press elsewhere (but on
    /// Import, which closes it itself) closes it.
    fn import_menu(&mut self, ui: &Ui, look: &mut Look, import: Rect) {
        let title = "Image source options";
        let title_height =
            ui.painter().layout_no_wrap(title.to_owned(), Weight::Bold.font(22.0), Color32::WHITE).size().y;
        let height = panel::ARROW_HEIGHT + 16.0 + title_height + 12.0 + 3.0 * 44.0 + 2.0 + 20.0 + 44.0 + 8.0;
        let rect = Rect::from_min_size(pos2(import.center().x - 160.0, import.top() + 47.0), vec2(320.0, height));
        // nothing under it is touched
        let _ = ui.interact(rect, ui.scope_id().with("import menu"), Sense::click());
        let body = panel::solid_frame(ui, look, "source import", rect, 160.0, false, 24.0, LIGHT_PANEL);
        let at = pos2(body.left() + 8.0 + 12.0, body.top() + 16.0);
        text(ui.painter(), at, Align2::LEFT_TOP, title, 22.0, Weight::Bold, argb(LIGHT_TEXT));
        let first = Rect::from_min_size(
            pos2(body.left() + 8.0, body.top() + 16.0 + title_height + 12.0),
            vec2(body.width() - 16.0, 3.0 * 44.0 + 2.0),
        );
        let second = Rect::from_min_size(pos2(first.left(), first.bottom() + 20.0), vec2(first.width(), 44.0));
        ui.painter().rect_filled(first, 14, argb(LIGHT_ROW));
        ui.painter().rect_filled(second, 14, argb(LIGHT_ROW));
        let mut picked = None;
        for (k, (icon, label, size)) in IMPORTS.iter().enumerate() {
            let row = if k < 3 {
                Rect::from_min_size(pos2(first.left(), first.top() + 45.0 * k as f32), vec2(first.width(), 44.0))
            } else {
                second
            };
            if k > 0 && k < 3 {
                let line = Rect::from_min_size(pos2(row.left() + 52.0, row.top() - 1.0), vec2(row.width() - 52.0, 1.0));
                ui.painter().rect_filled(line, 0, argb(LIGHT_SEPARATOR));
            }
            look.icon(ui, icon, pos2(row.left() + 18.0 + 12.0, row.center().y), *size, argb(MENU_BLUE));
            let at = pos2(row.left() + 18.0 + 24.0 + 22.0, row.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(LIGHT_TEXT));
            if ui.interact(row, ui.scope_id().with(("import", k)), Sense::click()).clicked() {
                picked = Some(k);
            }
        }
        let opened = self.importing.unwrap_or(0);
        let pressed_elsewhere = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !import.contains(p))
        });
        if pressed_elsewhere && ui.ctx().cumulative_frame_nr() != opened {
            self.importing = None;
        }
        let Some(k) = picked else { return };
        self.importing = None;
        match k {
            0 | 1 => {
                let picked = picked_picture(if k == 0 { "Import a photo" } else { "Import a file" });
                self.take(picked);
            }
            2 => self.library = Some(Library::new(ui.ctx(), self.grain())),
            _ => self.take(pasted()),
        }
    }

    /// The source library: a sheet over everything with its title and Cancel, Shape Source and
    /// Grain Source with a search by name, then the shipped images three across with their names.
    /// A pick of the other kind than the one being edited isn't taken.
    fn library_view(&mut self, ui: &mut Ui, screen: Rect) {
        let grain = self.grain();
        let Some(library) = self.library.as_mut() else { return };
        ui.painter().rect_filled(screen, 0, argb(LIBRARY_VEIL));
        let _ = ui.interact(screen, ui.scope_id().with("library veil"), Sense::click_and_drag());
        let sheet = Rect::from_min_size(pos2(screen.center().x - 290.0, screen.top() + 74.0), vec2(580.0, 575.0));
        ui.painter().rect_filled(sheet, 26, argb(LIBRARY_SHEET));
        let head = Rect::from_min_size(pos2(sheet.left() + 19.0, sheet.top() + 69.0), vec2(sheet.width() - 38.0, 39.0));
        let at = pos2(head.left(), head.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, "Source Library", 32.0, Weight::Bold, argb(LIBRARY_TITLE));
        let at = pos2(head.right(), head.center().y);
        let cancel = text(ui.painter(), at, Align2::RIGHT_CENTER, "Cancel", 17.0, Weight::Regular, argb(LIBRARY_DIM));
        let mut close = ui.interact(cancel.expand(8.0), ui.scope_id().with("library cancel"), Sense::click()).clicked();
        let tabs = Rect::from_min_size(pos2(sheet.left() + 43.0, head.bottom() + 25.0), vec2(0.0, 45.0));
        for (k, label) in ["Shape Source", "Grain Source"].into_iter().enumerate() {
            let tab =
                Rect::from_min_size(pos2(tabs.left() + k as f32 * (115.0 + 11.0), tabs.top() + 2.0), vec2(115.0, 41.0));
            let on = library.grains == (k == 1);
            let ink = if on { argb(pc::MENU_INK) } else { argb(LIBRARY_DIM) };
            text(ui.painter(), tab.center(), Align2::CENTER_CENTER, label, 17.0, Weight::Regular, ink);
            if ui.interact(tab, ui.scope_id().with(("library tab", k)), Sense::click()).clicked() {
                library.grains = k == 1;
            }
        }
        let search = Rect::from_min_size(pos2(tabs.left() + 2.0 * (115.0 + 11.0) + 8.0, tabs.top()), vec2(225.0, 45.0));
        search_field(ui, search, &mut library.query);
        let query = library.query.trim().to_lowercase();
        let all = if library.grains { &library.grain_images } else { &library.shapes };
        let shown: Vec<(usize, &(String, TextureHandle))> = all
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| query.is_empty() || name.to_lowercase().contains(&query))
            .collect();
        let grid =
            Rect::from_min_max(pos2(sheet.left() + 16.0, tabs.bottom()), pos2(sheet.right() - 16.0, sheet.bottom()));
        let step = (grid.width() - 2.0 * CELL_GAP) / 3.0 + CELL_GAP;
        let mut picked = None;
        ui.scope_builder(UiBuilder::new().max_rect(grid), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("source library", library.grains))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(11.0);
                    for (r, row) in shown.chunks(3).enumerate() {
                        if r > 0 {
                            ui.add_space(CELL_ROW_GAP);
                        }
                        let (line, _) = ui.allocate_exact_size(vec2(grid.width(), CELL_HEIGHT), Sense::hover());
                        for (c, (index, (name, texture))) in row.iter().enumerate() {
                            let square =
                                Rect::from_min_size(pos2(line.left() + step * c as f32, line.top()), vec2(CELL, 146.0));
                            ui.painter().rect_filled(square, 8, Color32::BLACK);
                            // filling the box, cropped
                            let crop = (1.0 - 146.0 / CELL) / 2.0;
                            let uv = Rect::from_min_max(pos2(0.0, crop), pos2(1.0, 1.0 - crop));
                            ui.painter()
                                .add(RectShape::filled(square, 8, Color32::WHITE).with_texture(texture.id(), uv));
                            ui.painter().rect_stroke(
                                square,
                                8,
                                Stroke::new(1.0, argb(LIBRARY_EDGE)),
                                egui::StrokeKind::Inside,
                            );
                            let label = Rect::from_min_size(pos2(square.left(), square.bottom()), vec2(CELL, 26.0));
                            let painter = ui.painter().with_clip_rect(label.intersect(ui.clip_rect()));
                            let at = pos2(square.left() + 3.0, square.bottom() + 5.0);
                            text(&painter, at, Align2::LEFT_TOP, name, 13.0, Weight::Bold, argb(LIBRARY_TITLE));
                            let cell = square.union(label);
                            // each kind takes its own: a grain picked in the shape editor is not taken
                            if ui.interact(cell, ui.scope_id().with(("library cell", *index)), Sense::click()).clicked()
                                && library.grains == grain
                            {
                                picked = Some(*index);
                            }
                        }
                    }
                    ui.add_space(24.0);
                });
        });
        if let Some(index) = picked {
            let images = procraft_core::source_library(grain);
            self.image = images.into_iter().nth(index).map(|i| i.pixels).unwrap_or_default();
            // the grains' Blank isn't a shipped grain
            self.shipped = if grain { index.checked_sub(1) } else { Some(index) }.map(|i| i as u32);
            self.stale = true;
            close = true;
        }
        if close {
            self.library = None;
        }
    }
}

impl Library {
    fn new(ctx: &egui::Context, grains: bool) -> Library {
        let load = |grain: bool| {
            procraft_core::source_library(grain)
                .into_iter()
                .filter_map(|image| Some((image.name, picture(ctx, &image.pixels, false, false)?)))
                .collect()
        };
        Library { grains, query: String::new(), shapes: load(false), grain_images: load(true) }
    }
}

/// A square greyscale image as the editor shows it: a `shape` white over what is behind, a grain
/// as its greys (`repeat`: tiling); none if it is empty or not square.
fn picture(ctx: &egui::Context, pixels: &[u8], shape: bool, repeat: bool) -> Option<TextureHandle> {
    let n = (pixels.len() as f64).sqrt() as usize;
    if n == 0 || n * n != pixels.len() {
        return None;
    }
    let rgba: Vec<u8> = pixels.iter().flat_map(|&v| if shape { [255, 255, 255, v] } else { [v, v, v, 255] }).collect();
    let image = egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba);
    let options = if repeat { egui::TextureOptions::LINEAR_REPEAT } else { egui::TextureOptions::LINEAR };
    Some(ctx.load_texture("source editor", image, options))
}

/// A slider of Auto repeat's (301 × 55): its name and value over a thin track with a white knob,
/// which follows the pointer. Returns the value set, while held.
fn repeat_slider(ui: &Ui, cell: Rect, label: &str, value: &str, at: f32) -> Option<f32> {
    let ink = argb(REPEAT_LABEL);
    let name = text(ui.painter(), cell.min + vec2(16.0, 6.0), Align2::LEFT_TOP, label, 15.0, Weight::Regular, ink);
    text(
        ui.painter(),
        pos2(cell.right() - 16.0, cell.top() + 6.0),
        Align2::RIGHT_TOP,
        value,
        15.0,
        Weight::Regular,
        ink,
    );
    let bar = Rect::from_min_max(
        pos2(cell.left() + 16.0, name.bottom() + 6.0),
        pos2(cell.right() - 16.0, name.bottom() + 26.0),
    );
    let track = Rect::from_center_size(bar.center(), vec2(bar.width(), 3.0));
    ui.painter().rect_filled(track, 1.5, argb(REPEAT_TRACK));
    let filled = Rect::from_min_size(track.min, vec2(bar.width() * at.clamp(0.0, 1.0), 3.0));
    ui.painter().rect_filled(filled, 1.5, argb(pc::ACCENT));
    let knob = Rect::from_min_size(
        pos2(bar.left() + (bar.width() - 16.0) * at.clamp(0.0, 1.0), bar.center().y - 5.0),
        vec2(16.0, 10.0),
    );
    ui.painter().rect_filled(knob, 5, Color32::WHITE);
    let response = ui.interact(bar, ui.scope_id().with(("repeat slider", label)), Sense::click_and_drag());
    if !response.is_pointer_button_down_on() {
        return None;
    }
    let p = response.interact_pointer_pos()?;
    Some(((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0))
}

/// The library's search: a round field with a magnifier and its hint, matching names as typed.
fn search_field(ui: &mut Ui, field: Rect, query: &mut String) {
    ui.painter().rect_filled(field, 22.5, argb(SEARCH_FILL));
    ui.painter().rect_stroke(field, 22.5, Stroke::new(1.0, argb(SEARCH_EDGE)), egui::StrokeKind::Inside);
    // a magnifier: a ring and its handle
    let glass = Rect::from_min_size(pos2(field.left() + 11.0, field.center().y - 11.0), vec2(23.0, 22.0));
    let c = glass.min + vec2(glass.width() * 0.44, glass.height() * 0.44);
    let hint = argb(SEARCH_HINT);
    ui.painter().circle_stroke(c, 7.0, Stroke::new(2.0, hint));
    let d = 7.0 * std::f32::consts::FRAC_1_SQRT_2;
    let (from, to) = (c + vec2(d, d), c + vec2(d + 5.0, d + 5.0));
    ui.painter().line_segment([from, to], Stroke::new(2.4, hint));
    ui.painter().circle_filled(to, 1.2, hint);
    let typing = Rect::from_min_max(pos2(glass.right() + 9.0, field.top()), pos2(field.right() - 12.0, field.bottom()));
    if query.is_empty() {
        let at = pos2(typing.left(), typing.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, "Search Library", 17.0, Weight::Regular, hint);
    }
    egui::TextEdit::singleline(query)
        .id(Id::unique("source library search"))
        .font(Weight::Regular.font(17.0))
        .text_color(Color32::WHITE)
        .frame(egui::Frame::NONE)
        .margin(vec2(0.0, 0.0))
        .align(Align2::LEFT_CENTER)
        .min_size(typing.size())
        .desired_width(typing.width())
        .char_limit(64)
        .show(&mut ui.new_child(UiBuilder::new().max_rect(typing)));
}

/// A picture the user picks, as a source image; none if they pick none or it can't be read.
fn picked_picture(title: &str) -> Option<Vec<u8>> {
    let path = rfd::FileDialog::new().set_title(title).add_filter("Pictures", PICTURES).pick_file()?;
    let picture = std::fs::read(&path).ok().and_then(|bytes| image::load_from_memory(&bytes).ok());
    let Some(picture) = picture else {
        crate::files::tell("Can't import it", "This picture can't be opened.");
        return None;
    };
    // a big picture made smaller first: the source is far smaller still
    let picture = if picture.width().max(picture.height()) > 2048 { picture.thumbnail(2048, 2048) } else { picture };
    let rgba = picture.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(procraft_core::source_from_picture(rgba.into_raw(), w, h))
}

/// A picture on the clipboard as a source image; none if there isn't one.
fn pasted() -> Option<Vec<u8>> {
    let image = arboard::Clipboard::new().ok().and_then(|mut c| c.get_image().ok());
    let Some(image) = image else {
        crate::files::tell("Can't paste", "There's no picture to paste.");
        return None;
    };
    Some(procraft_core::source_from_picture(image.bytes.into_owned(), image.width as u32, image.height as u32))
}
