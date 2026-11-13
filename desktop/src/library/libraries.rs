//! The brush libraries in the library panel: the title's menu, every library's card and its
//! menu, and a library with no sets yet.

use std::sync::Arc;

use egui::{Align2, Color32, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{BrushLibraryInfo, CanvasSession};

use super::{Deleting, Library, SET_CELL, one_line};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::menu::{self, Entry};
use crate::panel::{self, PANEL_GLYPH, PANEL_TITLE};

/// A library's card: its size, the gap between cards, its corner, and its sets' rows.
const CARD_HEIGHT: f32 = 490.0;
const CARD_GAP: f32 = 8.0;
const CARD_CORNER: u8 = 16;
const CARD_SET_ROW: f32 = 19.0;
const CARD_SET_GAP: f32 = 3.0;
/// A brush's row on a card, and how many cards' strokes a thread draws before the next takes over.
const CARD_BRUSH: f32 = 30.0;
const CARD_PREVIEWS_AT_A_TIME: usize = 12;
/// How often the panel looks for strokes drawn meanwhile.
const CARD_LOOK: std::time::Duration = std::time::Duration::from_millis(50);
/// A set's row on a card, not the one shown.
const CARD_SET_FILL: Themed = Themed(0x59000000, 0x14000000);

/// A card's stroke drawn on a thread: its library, set and place, and its picture (none: it can't
/// be drawn).
pub(super) type CardStroke = ((u32, u32, u32), Option<egui::ColorImage>);

impl Library {
    /// The cards' strokes forgotten, and any being drawn left to finish unread: the libraries
    /// changed.
    pub(super) fn forget_cards(&mut self) {
        self.card_previews.clear();
        self.card_wanted.clear();
        self.card_drawing = None;
    }

    /// Takes the cards' strokes drawn so far, and has those wanted drawn on a thread of their own.
    fn draw_card_previews(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if let Some(drawing) = &self.card_drawing {
            loop {
                match drawing.try_recv() {
                    Ok((key, image)) => {
                        let texture = image.map(|image| {
                            ctx.load_texture(format!("card brush {key:?}"), image, egui::TextureOptions::LINEAR)
                        });
                        self.card_previews.insert(key, texture);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.card_drawing = None;
                        break;
                    }
                }
            }
        }
        let previews = &self.card_previews;
        self.card_wanted.retain(|key| !previews.contains_key(key));
        if self.card_drawing.is_none() && !self.card_wanted.is_empty() {
            let (done, drawing) = std::sync::mpsc::channel();
            let batch: Vec<_> = self.card_wanted.drain(..self.card_wanted.len().min(CARD_PREVIEWS_AT_A_TIME)).collect();
            let session = Arc::clone(session);
            let started = std::thread::Builder::new().name("library cards".into()).spawn(move || {
                for key in batch {
                    let image = session.brush_library_preview(key.0, key.1, key.2, 212, 65).map(|t| {
                        egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba)
                    });
                    // (the libraries changed meanwhile: these are no longer wanted)
                    if done.send((key, image)).is_err() {
                        return;
                    }
                }
            });
            match started {
                Ok(_) => self.card_drawing = Some(drawing),
                Err(e) => log::warn!("the library cards' strokes aren't drawn: {e}"),
            }
        }
        if self.card_drawing.is_some() {
            ctx.request_repaint_after(CARD_LOOK);
        }
    }

    /// The panel's lists' place (`lists`) showing every library's card, or, with the one in use
    /// empty, that it has no brushes yet.
    pub(super) fn libraries_body(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        libraries: &[BrushLibraryInfo],
        lists: Rect,
        shown: usize,
    ) {
        if !self.showing_libraries {
            let middle = pos2(lists.center().x, lists.top() + 60.0);
            text(
                ui.painter(),
                middle,
                Align2::CENTER_TOP,
                "This library has no brushes yet",
                17.0,
                Weight::SemiBold,
                argb(PANEL_TITLE),
            );
            text(
                ui.painter(),
                middle + vec2(0.0, 30.0),
                Align2::CENTER_TOP,
                "Make a set with +, then brushes in it.",
                14.0,
                Weight::Regular,
                argb(PANEL_GLYPH),
            );
            return;
        }
        let mut cards = Vec::new();
        let mut opened = None;
        let mut menu_for = None;
        ui.scope_builder(UiBuilder::new().max_rect(lists), |ui| {
            ScrollArea::vertical()
                .id_salt("brush libraries")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    let width = (lists.width() - 24.0 - CARD_GAP) / 2.0;
                    let rows = libraries.len().div_ceil(2);
                    let height = rows as f32 * (CARD_HEIGHT + CARD_GAP) - CARD_GAP + 12.0;
                    let (grid, _) = ui.allocate_exact_size(vec2(lists.width(), height), Sense::hover());
                    for (i, library) in libraries.iter().enumerate() {
                        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
                        let card = Rect::from_min_size(
                            pos2(
                                grid.left() + 12.0 + col * (width + CARD_GAP),
                                grid.top() + row * (CARD_HEIGHT + CARD_GAP),
                            ),
                            vec2(width, CARD_HEIGHT),
                        );
                        cards.push((i, card));
                        let response = ui.interact(card, ui.scope_id().with(("library card", i)), Sense::click());
                        self.library_card(ui, look, session, i, library, card, shown);
                        if response.secondary_clicked() || response.long_touched() {
                            menu_for = Some(i);
                        } else if response.clicked() {
                            opened = Some(i);
                        }
                    }
                });
        });
        self.card_rects = cards;
        self.draw_card_previews(ui.ctx(), session);
        if let Some(i) = menu_for {
            self.card_menu_for = Some(i);
            self.card_menu.open();
        } else if let Some(i) = opened {
            self.open_library(session, libraries, i);
        }
    }

    /// Brings library `i` into use and shows it.
    fn open_library(&mut self, session: &CanvasSession, libraries: &[BrushLibraryInfo], i: usize) {
        self.showing_libraries = false;
        if libraries.get(i).is_some_and(|l| l.current) {
            return;
        }
        if session.open_brush_library(i as u32) {
            self.shown_set = None;
            self.library_changed();
        } else {
            log::error!("brush library {i} couldn't be opened");
        }
    }

    /// One library's card: its name over a small likeness of it, its sets down the left and the
    /// brushes of one of them beside them; the one in use outlined.
    #[allow(clippy::too_many_arguments)]
    fn library_card(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        i: usize,
        library: &BrushLibraryInfo,
        card: Rect,
        shown: usize,
    ) {
        let painter = ui.painter_at(card);
        painter.rect_filled(card, CARD_CORNER, argb(SET_CELL));
        if library.current {
            painter.rect_stroke(card, CARD_CORNER, Stroke::new(2.0, argb(pc::ACCENT)), egui::StrokeKind::Inside);
        }
        let name_at = Rect::from_min_size(card.min + vec2(10.0, 10.0), vec2(card.width() - 20.0, 18.0));
        if !self.rename_field(ui, session, i, name_at, 14.0) {
            let name = one_line(ui, &library.name, 14.0, Weight::SemiBold, argb(PANEL_TITLE), name_at.width());
            painter.galley(name_at.min, name, argb(PANEL_TITLE));
        }
        // the one in use's shown set; another's first with a brush chosen in it
        let chosen =
            if library.current { shown } else { library.sets.iter().position(|s| s.chosen.is_some()).unwrap_or(0) };
        let sets_width = (card.width() - 20.0) * 0.45;
        let mut y = card.top() + 38.0;
        for (s, set) in library.sets.iter().enumerate() {
            if y + CARD_SET_ROW > card.bottom() - 8.0 {
                break;
            }
            let row = Rect::from_min_size(pos2(card.left() + 10.0, y), vec2(sets_width, CARD_SET_ROW));
            let selected = s == chosen;
            painter.rect_filled(row, 5, panel::row_fill(selected, CARD_SET_FILL));
            let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
            let icon = icons::SET_ICONS.get(set.icon as usize).unwrap_or(&icons::BRUSH_SET);
            look.icon(ui, icon, pos2(row.left() + 10.0, row.center().y), 18.0, ink);
            let name = one_line(ui, &set.name, 8.0, Weight::Regular, ink, row.width() - 22.0);
            painter.galley(pos2(row.left() + 20.0, row.center().y - name.size().y / 2.0), name, ink);
            y += CARD_SET_ROW + CARD_SET_GAP;
        }
        let left = card.left() + 10.0 + sets_width + 6.0;
        let width = card.right() - 10.0 - left;
        let mut y = card.top() + 38.0;
        for b in library.sets.get(chosen).map(|s| s.brushes.as_slice()).unwrap_or_default() {
            if y + CARD_BRUSH > card.bottom() - 8.0 {
                break;
            }
            // its name over its stroke, drawn small
            let row = Rect::from_min_size(pos2(left, y), vec2(width, CARD_BRUSH));
            let key = (i as u32, b.set, b.index);
            if !self.card_previews.contains_key(&key) && !self.card_wanted.contains(&key) {
                self.card_wanted.push(key);
            }
            if let Some(Some(stroke)) = self.card_previews.get(&key) {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let tint = if crate::look::light() { Color32::BLACK } else { Color32::WHITE };
                painter.image(stroke.id(), row, uv, tint);
            }
            let name = one_line(ui, &b.name, 8.0, Weight::Regular, argb(PANEL_TITLE), width - 4.0);
            painter.galley(row.min + vec2(3.0, 1.0), name, argb(PANEL_TITLE));
            y += CARD_BRUSH + 2.0;
        }
    }

    /// While library `i` is renamed, its name typed in place at `at`; true if it is being.
    pub(super) fn rename_field(&mut self, ui: &mut Ui, session: &CanvasSession, i: usize, at: Rect, size: f32) -> bool {
        let Some((_, typed, focused)) = self.renaming_library.as_mut().filter(|(r, _, _)| *r == i) else {
            return false;
        };
        let mut output = egui::TextEdit::singleline(typed)
            .font(Weight::SemiBold.font(size))
            .text_color(argb(PANEL_TITLE))
            .frame(egui::Frame::NONE)
            .margin(vec2(0.0, 0.0))
            .desired_width(at.width())
            .show(&mut ui.new_child(UiBuilder::new().max_rect(at)));
        if !*focused {
            // the whole name chosen, to type over
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
            self.renaming_library = None;
            session.rename_brush_library(i as u32, name);
        }
        true
    }

    /// The title's menu, hanging under it: in a library, Rename it, Share it or go back to every
    /// library; with every library shown, put the shipped one back.
    pub(super) fn title_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        title: Rect,
        libraries: &[BrushLibraryInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let ink = menu::INK;
        let entries: Vec<Entry> = if self.showing_libraries {
            vec![Entry::Item(&icons::MENU_RESTORE, "Restore default library", ink)]
        } else {
            vec![
                Entry::Item(&icons::MENU_RENAME, "Rename", ink),
                Entry::Item(&icons::MENU_SHARE, "Share", ink),
                Entry::Rule(11.0),
                Entry::Item(&icons::MENU_LIBRARIES, "Back to libraries", ink),
            ]
        };
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(title.left(), title.bottom()), size);
        let mut tapped = None;
        self.title_menu.show(ctx, "library title menu", rect, rect.left_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.title_menu.close();
        match *label {
            "Rename" => {
                let place = libraries.iter().position(|l| l.current).unwrap_or(0);
                let name = libraries.get(place).map(|l| l.name.clone()).unwrap_or_default();
                self.renaming_library = Some((place, name, false));
            }
            "Share" => {
                let name = libraries.iter().find(|l| l.current).map_or("Brushes", |l| l.name.as_str());
                match session.brush_library_as_file() {
                    Some(bytes) => crate::files::save_file(&bytes, name, "brushlibrary", "Brush library"),
                    None => crate::files::say("This library has no brushes to share."),
                }
            }
            "Back to libraries" => self.showing_libraries = true,
            _ => {
                self.forget_cards();
                if session.restore_stock_brush_library().is_none() {
                    log::error!("the shipped brush library couldn't be put back");
                }
            }
        }
    }

    /// The plus's menu with every library shown: a new library, or one from a file.
    pub(super) fn libraries_plus_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        plus: Rect,
        backdrops: &mut Vec<f32>,
    ) {
        let entries = [
            Entry::Item(&icons::MENU_NEW_LIBRARY, "Create new library", menu::INK),
            Entry::Item(&icons::MENU_FOLDER, "Import from Files", menu::INK),
        ];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(plus.right() - size.x, plus.bottom()), size);
        let mut tapped = None;
        self.libraries_plus.show(
            ctx,
            "libraries plus menu",
            rect,
            rect.right_top(),
            menu::OVER_A_PANEL,
            backdrops,
            |ui| {
                tapped = menu::entries(ui, look, rect, &entries);
            },
        );
        let Some(tapped) = tapped else { return };
        self.libraries_plus.close();
        self.forget_cards();
        if tapped == 0 {
            if session.new_brush_library().is_none() {
                log::error!("a new brush library couldn't be made");
            }
        } else if let Some(file) = crate::files::pick_file("Import a brush library") {
            // read on a thread of its own, then added as a library and brought into use
            self.add_brushes(vec![file], true);
        }
    }

    /// A card's menu, over its upper part: Rename, Duplicate, and Delete while there are others.
    pub(super) fn card_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        libraries: &[BrushLibraryInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let Some(i) = self.card_menu_for else { return };
        let Some(card) = self.card_rects.iter().find(|(c, _)| *c == i).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let mut entries = vec![
            Entry::Item(&icons::MENU_RENAME, "Rename", ink),
            Entry::Item(&icons::MENU_DUPLICATE, "Duplicate", ink),
        ];
        if libraries.len() > 1 {
            entries.push(menu::DIVIDER);
            entries.push(Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER));
        }
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(card.center().x - size.x / 2.0, card.top() + card.height() / 4.0), size);
        let mut tapped = None;
        self.card_menu.show(ctx, "library card menu", rect, rect.center_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.card_menu.close();
        match *label {
            "Rename" => {
                let name = libraries.get(i).map(|l| l.name.clone()).unwrap_or_default();
                self.renaming_library = Some((i, name, false));
            }
            "Duplicate" => {
                self.forget_cards();
                if session.duplicate_brush_library(i as u32).is_none() {
                    log::error!("brush library {i} couldn't be copied");
                }
            }
            _ => {
                self.delete_what = Some(Deleting::Library(i as u32));
                self.deleting.open(
                    "Delete this library?",
                    "Its sets and brushes go with it.",
                    "Delete",
                    Some(&icons::TRASH),
                );
            }
        }
    }
}
