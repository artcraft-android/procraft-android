//! The gallery, as the Android app's (ui/GalleryScreen.kt): every artwork as a thumbnail with its
//! name and size, a stack as its top artwork over two turned cards; over them the app's name (in a
//! stack, the way back and its name), Select, Import, Photo and +, which opens the New canvas list
//! (the window's size, then the presets). Select picks artworks to stack, preview, duplicate or
//! delete. The artworks are the core's store (gallery.rs), kept in the app's folder; the presets
//! are the core's too, kept as the Android app keeps them.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, TextureHandle, Ui, UiBuilder, pos2, vec2,
};
use procraft_core::{
    ArtworkStore, CanvasPreset, GalleryArtwork, GalleryEntry, P3, preset_label, presets_from_json, profile_chip,
};

use crate::icons;
use crate::look::{Look, Weight, argb, text};
use crate::panel;

pub const BACKGROUND: u32 = 0xFF212121;
const TITLE: u32 = 0xFFF0F0F0;
const TEXT: u32 = 0xFFAAAAAA;
const SIZE: u32 = 0xFF666666;
const CAPSULE: u32 = 0xFF1C1C1C;
const CAPSULE_EDGE: u32 = 0xFF393939;
const HEADER: f32 = 55.0;
/// The New canvas list: light, hanging from +.
const NEW_WIDTH: f32 = 347.0;
const LIGHT_PANEL: u32 = 0xFFD5D5D5;
const LIGHT_TEXT: u32 = 0xFF393939;
const LIGHT_ROW_STRONG: u32 = 0xFFE1E1E1;
const LIGHT_ROW: u32 = 0xFFDEDEDE;
const LIGHT_SEPARATOR: u32 = 0xFFD2D2D2;
const LIGHT_CHIP: u32 = 0xFFD6D6D6;
/// "New canvas" (21 above it, 20 below), and the presets' most before they scroll.
const NEW_HEADER: f32 = 21.0 + 31.0 + 20.0;
const PRESETS_MOST: f32 = 376.0;
/// A preset row swiped left: how far it slides, and the Edit and Delete it shows.
const PRESET_REVEAL: f32 = 159.0;
const PRESET_BEHIND: u32 = 0xFFD6D6D6;
const PRESET_EDIT: u32 = 0xFFC6C6C6;
const PRESET_EDIT_INK: u32 = 0xFF575757;
const PRESET_DELETE: u32 = 0xFFFE604D;
/// Select: the round button that ends it, a picked artwork's mark, the More list.
const DONE_FILL: u32 = 0xFF0190FF;
const PICKED: u32 = 0xFF0F7EFD;
const UNPICKED_RING: u32 = 0xFFCCCCCC;
const POPOVER: u32 = 0xFF212121;
const ACTIONS_GROUP: u32 = 0x420C0C0C;
const ACTIONS_SEPARATOR: u32 = 0x06FFFFFF;
const ACTIONS_DISABLED: u32 = 0xFF5E5E5E;
const PANEL_TITLE: u32 = 0xFFC7C7C7;
const PREVIEW_CLOSE: u32 = 0xFFD7D7D7;
/// Renaming: the gallery faded back, the artwork fitted in a 281 x 280 frame 20 from the top (12
/// in), the name's field 20 under it.
const RENAME_VEIL: u32 = 0xE6212121;
/// About: the veil over the gallery, the mark's field, the version's ink, the close button.
const ABOUT_VEIL: u32 = 0xD90B0B0B;
const ABOUT_FIELD: u32 = 0xFF0B0B0C;
const ABOUT_INK: u32 = 0xFFBDBDBD;
const ABOUT_CLOSE: u32 = 0xFFE3E3E3;
const ABOUT_CLOSE_INK: u32 = 0xFF1C1C1C;
const RENAME_FRAME: (f32, f32) = (281.0, 280.0);
const RENAME_TOP: f32 = 20.0;
const RENAME_MARGIN: f32 = 12.0;
const RENAME_GAP: f32 = 20.0;
const RENAME_FIELD: (f32, f32) = (262.0, 38.0);
const RENAME_FIELD_FILL: u32 = 0x14FFFFFF;
/// A swiped artwork's actions: three pills over its thumbnail, which fades back.
const SWIPE_PILL: (f32, f32) = (94.0, 56.0);
const SWIPE_PILL_FILL: u32 = 0xDB1C1C1C;
const SWIPE_PILL_INK: u32 = 0xFFA9A9A9;
const SWIPE_DELETE_FILL: u32 = 0xFFFE604D;
const SWIPED_FADE: f32 = 0.64;
/// Held this long, an artwork (or a stack) lifts to be dragged; while it is, its cell stays behind
/// faint, and what it would be stacked onto is washed blue.
const HOLD_S: f64 = 0.4;
const DRAGGED_FADE: f32 = 0.7;
const DROP_WASH: u32 = 0x800064DC;
const PREVIEW_ARROW: u32 = 0xFFB4B4B4;

/// One of Select's actions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectAction {
    Stack,
    Preview,
    Share,
    Duplicate,
    Delete,
    More,
}

/// What a tap in the gallery asked for.
pub enum Asked {
    /// The artwork in this folder, to open.
    Open(String),
    /// A new artwork as the preset says, in this folder (the gallery's or a stack's).
    New(CanvasPreset, Option<String>),
    /// A new artwork the screen's size, in this folder.
    ScreenSize(Option<String>),
    Import(Option<String>),
    Photo(Option<String>),
    /// These artworks written as this format, where the user picks.
    Share(Vec<GalleryArtwork>, crate::files::Format),
}

/// Share's formats, as their list shows them.
const SHARE_FORMATS: [crate::files::Format; 7] = [
    crate::files::Format::Procraft,
    crate::files::Format::Package,
    crate::files::Format::Psd,
    crate::files::Format::Pdf,
    crate::files::Format::Jpeg,
    crate::files::Format::Png,
    crate::files::Format::Tiff,
];

pub struct Gallery {
    pub store: Arc<ArtworkStore>,
    entries: Vec<GalleryEntry>,
    /// The stack open: its folder and name.
    stack: Option<(String, String)>,
    /// Each artwork's thumbnail (by folder), and when the artwork was changed when it was read.
    thumbs: HashMap<String, (i64, Option<TextureHandle>)>,
    new_open: bool,
    presets: Vec<CanvasPreset>,
    /// The screen's size, pixels: Screen Size's canvas.
    pub screen_px: (u32, u32),
    /// Select: on, the artworks picked (by folder), More's list open, the picked artworks shown
    /// one at a time (and which), and the alert asking before they are deleted.
    selecting: bool,
    picked: Vec<String>,
    more_open: bool,
    preview: Option<(Vec<GalleryArtwork>, usize, bool, f32)>,
    alert: crate::alert::Alert,
    /// The artwork being renamed, its name as typed, and whether its field has had the focus.
    renaming: Option<(GalleryArtwork, String, bool)>,
    /// The artwork (by folder) whose actions a swipe left shows, and the one asked about deleting.
    swiped: Option<String>,
    deleting: Option<String>,
    /// How far the drag on a cell has gone.
    dragged: egui::Vec2,
    /// Share's formats, open for these artworks, beside a swiped artwork's Share (or under
    /// Select's, where it last was).
    sharing: Option<(Vec<GalleryArtwork>, Option<Rect>)>,
    share_button: Rect,
    /// What a hold lifted (by folder) and where it was taken (window points), and where the way
    /// back out of a stack is, to drop it there.
    lifted: Option<(String, Pos2)>,
    back: Rect,
    /// Custom Canvas, open on a new canvas or a preset.
    custom: Option<crate::custom_canvas::CustomCanvas>,
    /// The preset row swiped open, and the one being dragged (and how far it is).
    preset_open: Option<usize>,
    preset_drag: Option<(usize, f32)>,
    /// What the app is, over the gallery, and its mark's picture once read.
    about: bool,
    mark: Option<TextureHandle>,
}

impl Gallery {
    pub fn new() -> Gallery {
        let store = ArtworkStore::open(crate::files::app_dir().join("artworks").to_string_lossy().into_owned());
        let presets = presets_from_json(std::fs::read_to_string(presets_path()).ok());
        let mut gallery = Gallery {
            store,
            entries: Vec::new(),
            stack: None,
            thumbs: HashMap::new(),
            new_open: false,
            presets,
            screen_px: (1920, 1080),
            selecting: false,
            picked: Vec::new(),
            more_open: false,
            preview: None,
            alert: crate::alert::Alert::default(),
            renaming: None,
            swiped: None,
            deleting: None,
            dragged: egui::Vec2::ZERO,
            sharing: None,
            share_button: Rect::NOTHING,
            lifted: None,
            back: Rect::NOTHING,
            custom: None,
            preset_open: None,
            preset_drag: None,
            about: false,
            mark: None,
        };
        gallery.refresh();
        gallery
    }

    /// Reads what is in the gallery (or the stack open) again.
    pub fn refresh(&mut self) {
        self.entries = self.store.list(self.stack.as_ref().map(|s| s.0.clone()));
        // the stack open is gone (its last artwork taken out): back to the gallery
        if self.stack.is_some() && self.entries.is_empty() {
            self.stack = None;
            self.entries = self.store.list(None);
        }
    }

    fn keep_presets(&self) {
        if let Err(e) = std::fs::write(presets_path(), procraft_core::presets_json(self.presets.clone())) {
            log::warn!("keeping the presets: {e}");
        }
    }

    /// The folder new artworks go in: the stack open's, or the gallery's.
    fn folder(&self) -> Option<String> {
        self.stack.as_ref().map(|s| s.0.clone())
    }

    /// Shows the gallery over the whole window; returns what a tap asked for.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, colours: &crate::colours::Colours) -> Option<Asked> {
        let screen = ctx.content_rect();
        let shown =
            egui::Area::new(Id::unique("gallery")).fixed_pos(screen.min).order(Order::Background).show(ctx, |ui| {
                ui.allocate_rect(screen, Sense::hover());
                ui.painter().rect_filled(screen, 0, argb(BACKGROUND));
                let tapped = self.header(ui, look, screen);
                let grid = Rect::from_min_max(pos2(screen.left(), screen.top() + HEADER), screen.max);
                if self.entries.is_empty() {
                    let at = grid.center();
                    text(
                        ui.painter(),
                        at,
                        Align2::CENTER_CENTER,
                        "Tap + to start a new canvas",
                        17.0,
                        Weight::Regular,
                        argb(TEXT),
                    );
                    tapped
                } else {
                    self.grid(ui, grid).or(tapped)
                }
            });
        let mut asked = shown.inner;
        if self.new_open && self.custom.is_none() {
            asked = self.new_canvas(ctx, look, screen).or(asked);
        }
        if let Some(custom) = self.custom.as_mut() {
            use crate::custom_canvas::Done;
            match custom.show(ctx, look, &colours.palettes, colours.harmony) {
                Some(Done::Cancel) => self.custom = None,
                // an edited preset is kept in its place; a new one goes at the end of the list
                // and is made into an artwork
                Some(Done::Made(p)) => {
                    let editing = custom.editing;
                    self.custom = None;
                    match editing.and_then(|i| self.presets.get_mut(i)) {
                        Some(kept) => *kept = p,
                        None => {
                            self.presets.push(p.clone());
                            self.new_open = false;
                            asked = Some(Asked::New(p, self.folder()));
                        }
                    }
                    self.keep_presets();
                }
                None => {}
            }
        }
        if self.more_open {
            self.more(ctx, look, screen);
        }
        if self.about {
            self.about(ctx, screen);
        }
        if self.preview.is_some() {
            self.preview(ctx, look, screen);
        }
        if self.renaming.is_some() {
            self.rename(ctx, screen);
        }
        if self.sharing.is_some() {
            asked = self.formats(ctx, screen).or(asked);
        }
        match self.alert.show(ctx, look) {
            Some(crate::alert::Answer::Action) => {
                if let Some(dir) = self.deleting.take() {
                    self.store.delete(dir);
                    self.refresh();
                } else {
                    for dir in std::mem::take(&mut self.picked) {
                        self.store.delete(dir);
                    }
                    self.end_selecting();
                }
            }
            Some(_) => self.deleting = None,
            None => {}
        }
        asked
    }

    /// The artworks picked, in the gallery's order.
    fn picked_artworks(&self) -> Vec<GalleryArtwork> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                GalleryEntry::Artwork { artwork } if self.picked.contains(&artwork.dir) => Some(artwork.clone()),
                _ => None,
            })
            .collect()
    }

    fn end_selecting(&mut self) {
        self.selecting = false;
        self.picked.clear();
        self.more_open = false;
        self.refresh();
    }

    /// Whether `action` can act on what is picked now.
    fn can(&self, action: SelectAction) -> bool {
        match action {
            SelectAction::Stack => self.stack.is_none() && self.picked.len() >= 2,
            SelectAction::More => true,
            _ => !self.picked.is_empty(),
        }
    }

    fn act(&mut self, action: SelectAction) {
        if !self.can(action) {
            return;
        }
        match action {
            SelectAction::Stack => {
                let dirs = self.picked_artworks().into_iter().map(|a| a.dir).collect();
                self.store.stack(dirs);
                self.end_selecting();
            }
            SelectAction::Preview => {
                self.more_open = false;
                self.preview = Some((self.picked_artworks(), 0, false, 0.0));
            }
            SelectAction::Duplicate => {
                for a in self.picked_artworks() {
                    self.store.duplicate(a.dir);
                }
                self.end_selecting();
            }
            SelectAction::Delete => {
                let n = self.picked.len();
                let title = if n == 1 { "Delete 1 artwork?".to_owned() } else { format!("Delete {n} artworks?") };
                self.alert.open(&title, "This can't be undone.", "Delete", Some(&icons::TRASH));
            }
            SelectAction::More => self.more_open = !self.more_open,
            // sharing artworks that aren't open is still to come here
            SelectAction::Share => self.sharing = Some((self.picked_artworks(), None)),
        }
    }

    /// Select's title row: the title, then (on a wide window) Stack, Preview, Share, Duplicate and
    /// Delete, or Share, Delete and More, each dimmed until it can act; then the blue round button
    /// that ends selecting.
    fn select_header(&mut self, ui: &Ui, look: &mut Look, screen: Rect) {
        let painter = ui.painter();
        let top = screen.top();
        let title = self.stack.as_ref().map_or("ProCraft", |s| s.1.as_str()).to_owned();
        text(
            painter,
            pos2(screen.left() + 32.0, top + 11.0),
            Align2::LEFT_TOP,
            &title,
            36.0,
            Weight::Bold,
            argb(TITLE),
        );
        let wide = screen.width() > screen.height();
        let actions: &[(SelectAction, &str, f32)] = if wide {
            &[
                (SelectAction::Stack, "Stack", 65.0),
                (SelectAction::Preview, "Preview", 80.0),
                (SelectAction::Share, "Share", 65.0),
                (SelectAction::Duplicate, "Duplicate", 91.0),
                (SelectAction::Delete, "Delete", 70.0),
            ]
        } else {
            &[
                (SelectAction::Share, "Share", 65.0),
                (SelectAction::Delete, "Delete", 70.0),
                (SelectAction::More, "More", 61.0),
            ]
        };
        let done = Rect::from_min_size(pos2(screen.right() - 20.0 - 44.0, top + 10.0), vec2(44.0, 45.0));
        let width: f32 = actions.iter().map(|a| a.2).sum();
        let capsule = Rect::from_min_size(pos2(done.left() - 13.0 - width, top + 10.0), vec2(width, 45.0));
        painter.rect_filled(capsule, 22.5, argb(CAPSULE));
        painter.rect_stroke(capsule, 22.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
        let mut x = capsule.left();
        let mut chosen = None;
        for &(action, label, w) in actions {
            let button = Rect::from_min_size(pos2(x, capsule.top()), vec2(w, 45.0));
            let ink = if self.can(action) { argb(TITLE) } else { argb(SIZE) };
            text(painter, button.center(), Align2::CENTER_CENTER, label, 16.0, Weight::Regular, ink);
            if action == SelectAction::Share {
                self.share_button = button;
            }
            if ui.interact(button, ui.scope_id().with(("select", label)), Sense::click()).clicked() {
                chosen = Some(action);
            }
            x += w;
        }
        painter.rect_filled(done, 22.0, argb(DONE_FILL));
        look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
        if ui.interact(done, ui.scope_id().with("select done"), Sense::click()).clicked() {
            self.end_selecting();
        }
        if let Some(action) = chosen {
            self.act(action);
        }
    }

    /// More options for the picked artworks, under More: Preview, Stack (two or more), Duplicate.
    fn more(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) {
        let width = 250.0;
        let more_x = screen.right() - 20.0 - 44.0 - 13.0 - 61.0 / 2.0;
        let left = (more_x - width / 2.0).clamp(screen.left() + 4.0, screen.right() - 4.0 - width);
        let height = panel::ARROW_HEIGHT + 14.0 + 29.0 + 12.0 + 44.0 * 3.0 + 2.0 + 12.0;
        let rect = Rect::from_min_size(pos2(left, screen.top() + 57.0), vec2(width, height));
        let mut chosen = None;
        let mut close = false;
        egui::Area::new(Id::unique("more options")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.interact(rect, ui.scope_id().with("panel"), Sense::click_and_drag());
            let body = panel::solid_frame(
                ui,
                look,
                "more options",
                rect,
                more_x - left,
                false,
                panel::POPOVER_CORNER,
                POPOVER,
            );
            let ink = argb(PANEL_TITLE);
            text(
                ui.painter(),
                pos2(body.left() + 24.0, body.top() + 14.0),
                Align2::LEFT_TOP,
                "More Options",
                24.0,
                Weight::Bold,
                ink,
            );
            let group = Rect::from_min_size(
                pos2(body.left() + 10.0, body.top() + 14.0 + 29.0 + 12.0),
                vec2(width - 20.0, 44.0 * 3.0 + 2.0),
            );
            ui.painter().rect_filled(group, 18.0, argb(ACTIONS_GROUP));
            for (i, (action, label)) in [
                (SelectAction::Preview, "Preview"),
                (SelectAction::Stack, "Stack"),
                (SelectAction::Duplicate, "Duplicate"),
            ]
            .into_iter()
            .enumerate()
            {
                let row =
                    Rect::from_min_size(pos2(group.left(), group.top() + 45.0 * i as f32), vec2(group.width(), 44.0));
                if i > 0 {
                    let line = Rect::from_min_size(pos2(row.left(), row.top() - 1.0), vec2(row.width(), 1.0));
                    ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
                }
                let on = self.can(action);
                let colour = if on { ink } else { argb(ACTIONS_DISABLED) };
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    colour,
                );
                if on && ui.interact(row, ui.scope_id().with(("more", i)), Sense::click()).clicked() {
                    chosen = Some(action);
                }
            }
        });
        if close {
            self.more_open = false;
        }
        if let Some(action) = chosen {
            self.more_open = false;
            self.act(action);
        }
    }

    /// Renaming an artwork: it shows at the top middle, larger, over the gallery faded back, with
    /// its name in a field under it, all of it chosen. Enter, Escape or a tap away keeps the name
    /// (an empty one keeps the old).
    fn rename(&mut self, ctx: &egui::Context, screen: Rect) {
        let Some((a, mut name, mut focused)) = self.renaming.take() else { return };
        let mut done = false;
        egui::Area::new(Id::unique("rename artwork")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            done = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.painter().rect_filled(screen, 0, argb(RENAME_VEIL));
            let (fw, fh) = (RENAME_FRAME.0 - RENAME_MARGIN * 2.0, RENAME_FRAME.1 - RENAME_MARGIN * 2.0);
            let ratio = if a.width > 0 && a.height > 0 { a.width as f32 / a.height as f32 } else { 0.75 };
            let (aw, ah) = if ratio > fw / fh { (fw, fw / ratio) } else { (fh * ratio, fh) };
            let top = screen.top() + RENAME_TOP + (RENAME_FRAME.1 - ah) / 2.0;
            let picture = Rect::from_min_size(pos2(screen.center().x - aw / 2.0, top), vec2(aw, ah));
            self.thumbnail(ui, picture, &a, 0.0);
            let field_top = screen.top() + RENAME_TOP + RENAME_FRAME.1 + RENAME_GAP;
            let field = Rect::from_min_size(
                pos2(screen.center().x - RENAME_FIELD.0 / 2.0, field_top),
                vec2(RENAME_FIELD.0, RENAME_FIELD.1),
            );
            ui.painter().rect_filled(field, 10.0, argb(RENAME_FIELD_FILL));
            let inner = field.shrink2(vec2(12.0, 0.0));
            let mut output = egui::TextEdit::singleline(&mut name)
                .font(Weight::Regular.font(17.0))
                .text_color(Color32::WHITE)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .align(Align2::CENTER_CENTER)
                .desired_width(inner.width())
                .char_limit(256)
                .show(&mut ui.new_child(
                    UiBuilder::new().max_rect(Rect::from_center_size(inner.center(), vec2(inner.width(), 22.0))),
                ));
            if !focused {
                output.response.request_focus();
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(name.chars().count()),
                );
                output.state.cursor.set_char_range(Some(all));
                output.state.store(ui.ctx(), output.response.id);
                focused = true;
            } else if output.response.lost_focus() {
                done = true;
            }
        });
        if done {
            let kept = if name.trim().is_empty() { a.name.clone() } else { name.trim().to_owned() };
            self.store.rename(a.dir, kept);
            self.refresh();
        } else {
            self.renaming = Some((a, name, focused));
        }
    }

    /// What the app is, over the gallery under a near-black veil: its mark 300 across in its
    /// field, its name and its version; the round button at the top right (or Escape) closes it.
    fn about(&mut self, ctx: &egui::Context, screen: Rect) {
        if self.mark.is_none() {
            let bytes = include_bytes!("../../android/app/src/main/res/drawable-nodpi/procraft_mark.png");
            let image = crate::files::png_rgba(bytes)
                .map(|(w, h, rgba)| egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
            self.mark = image.map(|i| ctx.load_texture("app mark", i, egui::TextureOptions::LINEAR));
        }
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::unique("about")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, argb(ABOUT_VEIL));
            let field = Rect::from_min_size(pos2(screen.center().x - 150.0, screen.top() + 95.0), vec2(300.0, 300.0));
            ui.painter().rect_filled(field, 67.0, argb(ABOUT_FIELD));
            if let Some(t) = &self.mark {
                // the mark drawn 450 across, the field showing its middle 300
                let uv = Rect::from_min_max(pos2(75.0 / 450.0, 75.0 / 450.0), pos2(375.0 / 450.0, 375.0 / 450.0));
                ui.painter().add(egui::epaint::RectShape::filled(field, 67, Color32::WHITE).with_texture(t.id(), uv));
            }
            let name = text(
                ui.painter(),
                pos2(field.center().x, field.bottom() + 12.0),
                Align2::CENTER_TOP,
                "ProCraft",
                64.0,
                Weight::Bold,
                Color32::WHITE,
            );
            let version = concat!("Version ", env!("CARGO_PKG_VERSION"));
            text(
                ui.painter(),
                pos2(field.center().x, name.bottom() + 14.0),
                Align2::CENTER_TOP,
                version,
                13.0,
                Weight::Regular,
                argb(ABOUT_INK),
            );
            let button = Rect::from_min_size(pos2(screen.right() - 31.5 - 22.5, screen.top() + 31.0), vec2(22.5, 22.5));
            ui.painter().circle_filled(button.center(), 11.25, argb(ABOUT_CLOSE));
            let (c, d) = (button.center(), 4.0);
            let stroke = Stroke::new(1.8, argb(ABOUT_CLOSE_INK));
            ui.painter().line_segment([c - vec2(d, d), c + vec2(d, d)], stroke);
            ui.painter().line_segment([c + vec2(d, -d), c + vec2(-d, d)], stroke);
            close |= ui.interact(button.expand(10.0), ui.scope_id().with("about close"), Sense::click()).clicked();
        });
        if close {
            self.about = false;
        }
    }

    /// The picked artworks one at a time over the whole window, on black: a drag across goes to
    /// the next or the last, a tap shows (or hides) the close button and the arrows.
    fn preview(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) {
        let Some((list, index, controls, moved)) = self.preview.take() else { return };
        let Some(a) = list.get(index).cloned() else { return };
        let (mut index, mut controls, mut moved, mut closed) = (index, controls, moved, false);
        egui::Area::new(Id::unique("preview")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, Color32::BLACK);
            if let Some(t) = self.texture(ctx, &a) {
                let picture = Self::fitted(screen, &a);
                let picture = picture.translate(vec2(0.0, screen.center().y - picture.center().y));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(t, picture, uv, Color32::WHITE);
            }
            if response.drag_started() {
                moved = 0.0;
            }
            moved += response.drag_delta().x;
            if response.drag_stopped() {
                if moved < -60.0 && index + 1 < list.len() {
                    index += 1;
                } else if moved > 60.0 && index > 0 {
                    index -= 1;
                }
            } else if response.clicked() {
                controls = !controls;
            }
            if controls {
                // a light disc with a black cross, 41 in from the top right
                let close = Rect::from_center_size(pos2(screen.right() - 41.0, screen.top() + 22.0), vec2(44.0, 44.0));
                ui.painter().circle_filled(close.center(), 23.5 / 2.0, argb(PREVIEW_CLOSE));
                look.icon(ui, &icons::CROSS, close.center(), 15.0, Color32::BLACK);
                closed = ui.interact(close, ui.scope_id().with("preview close"), Sense::click()).clicked();
                for (left, step) in [(true, -1i64), (false, 1)] {
                    let x = if left { screen.left() + 18.0 + 22.0 } else { screen.right() - 18.0 - 22.0 };
                    let button = Rect::from_center_size(pos2(x, screen.center().y), vec2(44.0, 60.0));
                    let (w, h) = (13.5, 23.5);
                    let (tip, back) = if left { (x - w / 2.0, x + w / 2.0) } else { (x + w / 2.0, x - w / 2.0) };
                    let y = screen.center().y;
                    let stroke = Stroke::new(4.0, argb(PREVIEW_ARROW));
                    ui.painter().line_segment([pos2(back, y - h / 2.0), pos2(tip, y)], stroke);
                    ui.painter().line_segment([pos2(tip, y), pos2(back, y + h / 2.0)], stroke);
                    if ui.interact(button, ui.scope_id().with(("preview arrow", left)), Sense::click()).clicked() {
                        index = (index as i64 + step).clamp(0, list.len() as i64 - 1) as usize;
                    }
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                closed = true;
            }
        });
        if !closed {
            self.preview = Some((list, index, controls, moved));
        }
    }

    /// The app's name (in a stack, the way back and its name), then Select, Import and Photo in a
    /// capsule, and +.
    fn header(&mut self, ui: &Ui, look: &mut Look, screen: Rect) -> Option<Asked> {
        if self.selecting {
            self.select_header(ui, look, screen);
            return None;
        }
        let painter = ui.painter();
        let top = screen.top();
        if let Some((_, name)) = self.stack.clone() {
            let row = Rect::from_min_size(pos2(screen.left() + 20.0, top + 12.0), vec2(40.0, 41.0));
            self.back = row.union(row.translate(vec2(300.0, 0.0)));
            let response = ui.interact(self.back, ui.scope_id().with("back"), Sense::click());
            // blue while touched, or while something dragged is over it, to take it out
            let over =
                self.lifted.is_some() && ui.input(|i| i.pointer.latest_pos()).is_some_and(|p| self.back.contains(p));
            let lit = response.is_pointer_button_down_on() || over;
            let ink = if lit { argb(crate::look::pc::ACCENT) } else { argb(TITLE) };
            look.icon(ui, &icons::CHEVRON_LEFT, row.center(), 28.0, ink);
            text(painter, pos2(row.right(), row.center().y), Align2::LEFT_CENTER, &name, 36.0, Weight::Bold, ink);
            if response.clicked() {
                self.stack = None;
                self.refresh();
            }
        } else {
            let name = text(
                painter,
                pos2(screen.left() + 32.0, top + 11.0),
                Align2::LEFT_TOP,
                "ProCraft",
                36.0,
                Weight::Bold,
                argb(TITLE),
            );
            // the app's name tapped: what it is and its version
            if ui.interact(name, ui.scope_id().with("about"), Sense::click()).clicked() {
                self.about = true;
            }
        }
        let plus = Rect::from_min_size(pos2(screen.right() - 20.0 - 44.0, top + 10.0), vec2(44.0, 45.0));
        let capsule_width = 69.0 + 71.0 + 66.0;
        let capsule =
            Rect::from_min_size(pos2(plus.left() - 12.0 - capsule_width, top + 10.0), vec2(capsule_width, 45.0));
        painter.rect_filled(capsule, 22.5, argb(CAPSULE));
        painter.rect_stroke(capsule, 22.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
        let mut x = capsule.left();
        let mut asked = None;
        for (label, width) in [("Select", 69.0), ("Import", 71.0), ("Photo", 66.0)] {
            let button = Rect::from_min_size(pos2(x, capsule.top()), vec2(width, 45.0));
            text(painter, button.center(), Align2::CENTER_CENTER, label, 16.0, Weight::Regular, argb(TEXT));
            if ui.interact(button, ui.scope_id().with(label), Sense::click()).clicked() {
                asked = match label {
                    "Import" => Some(Asked::Import(self.folder())),
                    "Photo" => Some(Asked::Photo(self.folder())),
                    _ => {
                        self.selecting = true;
                        self.new_open = false;
                        None
                    }
                };
            }
            x += width;
        }
        let disc = Rect::from_center_size(plus.center(), vec2(42.0, 42.0));
        painter.rect_filled(disc, 21.0, argb(CAPSULE));
        painter.rect_stroke(disc, 21.0, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
        look.icon(ui, &icons::ADD, plus.center(), 40.0, argb(TEXT));
        if ui.interact(plus, ui.scope_id().with("new canvas"), Sense::click()).clicked() {
            self.new_open = !self.new_open;
        }
        asked
    }

    /// The artworks and stacks, three across (four on a wide window), scrolling.
    fn grid(&mut self, ui: &mut Ui, rect: Rect) -> Option<Asked> {
        let columns = if rect.width() > rect.height() + HEADER { 4 } else { 3 };
        let pitch = rect.width() / columns as f32;
        let rows = self.entries.len().div_ceil(columns);
        let mut asked = None;
        let mut into_stack = false;
        let entries = self.entries.clone();
        // the cells laid out this frame, for where a drag would land
        let mut cells: Vec<(usize, Rect)> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ScrollArea::vertical().id_salt("gallery grid").show(ui, |ui| {
                let (all, _) = ui.allocate_exact_size(vec2(rect.width(), rows as f32 * pitch + 24.0), Sense::hover());
                for (i, entry) in entries.iter().enumerate() {
                    let (row, column) = (i / columns, i % columns);
                    let cell = Rect::from_min_size(
                        all.min + vec2(column as f32 * pitch, row as f32 * pitch),
                        vec2(pitch, pitch),
                    );
                    if !ui.is_rect_visible(cell) {
                        continue;
                    }
                    let response = ui.interact(cell, ui.scope_id().with(("gallery cell", i)), Sense::click_and_drag());
                    cells.push((i, cell));
                    // held still, it lifts; a lift let go where it was is no tap
                    if self.lifted.is_none()
                        && !self.selecting
                        && response.is_pointer_button_down_on()
                        && !response.dragged()
                    {
                        let held = ui.input(|inp| inp.pointer.press_start_time().map_or(0.0, |t| inp.time - t));
                        if held >= HOLD_S {
                            let at = response.interact_pointer_pos().unwrap_or_else(|| cell.center());
                            self.lifted = Some((entry_dir(entry).to_owned(), at));
                            self.swiped = None;
                        } else {
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(HOLD_S - held));
                        }
                    }
                    let tapped = response.clicked() && self.lifted.is_none();
                    match entry {
                        GalleryEntry::Artwork { artwork } => {
                            self.artwork_cell(ui, cell, artwork);
                            // under its thumbnail (on its name and size), a tap renames it
                            let below = cell.top() + 39.0 + Self::thumb_box(cell).height() + 8.0;
                            let on_name = response.interact_pointer_pos().is_some_and(|p| p.y > below);
                            // dragged across, its actions show (left) or go (right)
                            if response.drag_started() {
                                self.dragged = egui::Vec2::ZERO;
                            }
                            self.dragged += response.drag_delta();
                            if response.drag_stopped()
                                && !self.selecting
                                && self.lifted.is_none()
                                && self.dragged.x.abs() > self.dragged.y.abs()
                            {
                                self.swiped = (self.dragged.x < 0.0).then(|| artwork.dir.clone());
                            }
                            let swiped = self.swiped.as_ref() == Some(&artwork.dir);
                            if swiped {
                                self.swipe_actions(ui, cell, artwork);
                            }
                            // selecting, a tap picks it (or lets it go); a tap puts away actions
                            // showing
                            if tapped && self.swiped.is_some() {
                                self.swiped = None;
                            } else if tapped && on_name && !self.selecting {
                                self.renaming = Some((artwork.clone(), artwork.name.clone(), false));
                            } else if tapped && self.selecting {
                                if let Some(at) = self.picked.iter().position(|d| *d == artwork.dir) {
                                    self.picked.remove(at);
                                } else {
                                    self.picked.push(artwork.dir.clone());
                                }
                            } else if tapped {
                                asked = Some(Asked::Open(artwork.dir.clone()));
                            }
                        }
                        GalleryEntry::Stack { dir, name, artworks } => {
                            self.stack_cell(ui, cell, name, artworks);
                            if tapped && !self.selecting {
                                self.stack = Some((dir.clone(), name.clone()));
                                into_stack = true;
                            }
                        }
                    }
                }
            });
        });
        self.drag(ui, &entries, &cells);
        if into_stack {
            self.refresh();
        }
        asked
    }

    /// Where a point over the grid would put what is dragged: onto another's picture (an
    /// artwork's thumbnail as it sits in its box, or a stack's box), when it may be stacked; anywhere
    /// else the slot between cells it is nearest, before a cell's middle or after it.
    fn drop_at(
        entries: &[GalleryEntry],
        cells: &[(usize, Rect)],
        dragged: &str,
        p: Pos2,
        stacking: bool,
    ) -> Option<(Option<String>, usize)> {
        for &(i, cell) in cells {
            if !cell.contains(p) {
                continue;
            }
            let e = entries.get(i)?;
            let frame = Self::thumb_box(cell);
            let picture = match e {
                GalleryEntry::Artwork { artwork } => Self::fitted(frame, artwork),
                GalleryEntry::Stack { .. } => frame,
            };
            if stacking && entry_dir(e) != dragged && picture.contains(p) {
                return Some((Some(entry_dir(e).to_owned()), i));
            }
            return Some((None, if p.x < cell.center().x { i } else { i + 1 }));
        }
        // past the last cell: last
        let &(last, cell) = cells.last()?;
        let past = p.y >= cell.bottom() || (p.y >= cell.top() && p.x >= cell.right());
        (last + 1 == entries.len() && past).then_some((None, entries.len()))
    }

    /// A lifted artwork (or stack) follows the pointer, its cell faint behind; let go, it goes where
    /// it was let go: onto another to stack with it, between cells to move there, or (in a stack)
    /// on the way back, out to the gallery's front.
    fn drag(&mut self, ui: &Ui, entries: &[GalleryEntry], cells: &[(usize, Rect)]) {
        let Some((dir, from)) = self.lifted.clone() else { return };
        let Some(entry) = entries.iter().find(|e| entry_dir(e) == dir).cloned() else {
            self.lifted = None;
            return;
        };
        let (pointer, down) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.any_down()));
        let stacking = self.stack.is_none() && matches!(entry, GalleryEntry::Artwork { .. });
        let target = pointer.and_then(|p| Self::drop_at(entries, cells, &dir, p, stacking));
        for &(i, cell) in cells {
            if entries.get(i).is_some_and(|e| entry_dir(e) == dir) {
                ui.painter().rect_filled(cell, 0, argb(BACKGROUND).gamma_multiply(DRAGGED_FADE));
            }
            if let Some((Some(onto), _)) = &target
                && entries.get(i).is_some_and(|e| entry_dir(e) == onto)
            {
                let frame = Self::thumb_box(cell);
                let picture = match entries.get(i) {
                    Some(GalleryEntry::Artwork { artwork }) => Self::fitted(frame, artwork),
                    _ => frame,
                };
                ui.painter().rect_filled(picture, 8, argb(DROP_WASH));
            }
        }
        // the picture under the pointer, held where it was taken
        let shown = match &entry {
            GalleryEntry::Artwork { artwork } => Some(artwork.clone()),
            GalleryEntry::Stack { artworks, .. } => artworks.first().cloned(),
        };
        if let (Some(a), Some(p), Some(&(_, cell))) =
            (shown, pointer, cells.iter().find(|(i, _)| entries.get(*i).is_some_and(|e| entry_dir(e) == dir)))
        {
            let picture = Self::fitted(Self::thumb_box(cell), &a).translate(p - from);
            let texture = self.texture(ui.ctx(), &a);
            let painter = ui.ctx().layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("lifted artwork")));
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 24, spread: 0, color: Color32::from_black_alpha(0x80) };
            painter.add(shadow.as_shape(picture, 8));
            painter.rect_filled(picture, 8, Color32::WHITE);
            if let Some(t) = texture {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.add(egui::epaint::RectShape::filled(picture, 8, Color32::WHITE).with_texture(t, uv));
            }
        }
        if down {
            ui.ctx().request_repaint();
            return;
        }
        self.lifted = None;
        if self.stack.is_some() && pointer.is_some_and(|p| self.back.contains(p)) {
            self.store.move_entry(dir, 0, Some(self.store.root()));
        } else {
            match target {
                Some((Some(onto), _)) => self.store.put_onto(dir, onto),
                Some((None, slot)) => {
                    let at = entries.iter().position(|e| entry_dir(e) == dir).unwrap_or(slot);
                    let to = if slot > at { slot - 1 } else { slot };
                    if to != at {
                        self.store.move_entry(dir, to as i64, None);
                    }
                }
                None => {}
            }
        }
        self.refresh();
    }

    /// A swiped artwork's actions: Share, Duplicate and Delete as pills across the middle of its
    /// thumbnail's box, the cell faded back under them.
    fn swipe_actions(&mut self, ui: &Ui, cell: Rect, a: &GalleryArtwork) {
        ui.painter().rect_filled(cell, 0, argb(BACKGROUND).gamma_multiply(SWIPED_FADE));
        let frame = Self::thumb_box(cell);
        let width = SWIPE_PILL.0 * 3.0 + 6.0;
        let mut x = cell.center().x - width / 2.0;
        for (label, delete) in [("Share", false), ("Duplicate", false), ("Delete", true)] {
            let pill =
                Rect::from_min_size(pos2(x, frame.center().y - SWIPE_PILL.1 / 2.0), vec2(SWIPE_PILL.0, SWIPE_PILL.1));
            ui.painter().rect_filled(pill, 12.0, argb(if delete { SWIPE_DELETE_FILL } else { SWIPE_PILL_FILL }));
            let ink = if delete { Color32::WHITE } else { argb(SWIPE_PILL_INK) };
            text(ui.painter(), pill.center(), Align2::CENTER_CENTER, label, 15.0, Weight::SemiBold, ink);
            if ui.interact(pill, ui.scope_id().with(("swiped", label)), Sense::click()).clicked() {
                match label {
                    "Duplicate" => {
                        self.swiped = None;
                        self.store.duplicate(a.dir.clone());
                        self.refresh();
                    }
                    "Delete" => {
                        self.swiped = None;
                        self.deleting = Some(a.dir.clone());
                        let title = format!("Delete \u{201c}{}\u{201d}?", a.name);
                        self.alert.open(&title, "This can't be undone.", "Delete", Some(&icons::TRASH));
                    }
                    _ => {
                        self.swiped = None;
                        self.sharing = Some((vec![a.clone()], Some(pill)));
                    }
                }
            }
            x += SWIPE_PILL.0 + 3.0;
        }
    }

    /// Share's formats: beside a swiped artwork's Share where they fit (right, else left), or
    /// under Select's Share, kept inside the window; a tap on one shares in it, a tap off closes.
    fn formats(&mut self, ctx: &egui::Context, screen: Rect) -> Option<Asked> {
        let (list, from) = self.sharing.clone()?;
        let (width, n) = (260.0, SHARE_FORMATS.len() as f32);
        let height = 55.0 + 40.0 * n + (n - 1.0) + 10.0;
        let at = match from {
            Some(pill) => {
                let top = (pill.center().y - height / 2.0)
                    .clamp(screen.top() + 2.0, (screen.bottom() - 2.0 - height).max(2.0));
                let right = screen.right() - 2.0 - pill.right() - 2.0 >= width;
                pos2(if right { pill.right() + 2.0 } else { (pill.left() - 2.0 - width).max(screen.left() + 2.0) }, top)
            }
            None => {
                let centre = self.share_button.center().x;
                pos2(
                    (centre - width / 2.0).clamp(screen.left() + 4.0, screen.right() - 4.0 - width),
                    screen.top() + 69.0,
                )
            }
        };
        let rect = Rect::from_min_size(at, vec2(width, height));
        let (mut chosen, mut close) = (None, false);
        egui::Area::new(Id::unique("share formats")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.interact(rect, ui.scope_id().with("panel"), Sense::click_and_drag());
            let shadow =
                egui::epaint::Shadow { offset: [0, 0], blur: 51, spread: 0, color: argb(panel::POPOVER_SHADOW) };
            ui.painter().add(shadow.as_shape(rect, panel::POPOVER_CORNER));
            ui.painter().rect_filled(rect, panel::POPOVER_CORNER, argb(POPOVER));
            let ink = argb(PANEL_TITLE);
            text(
                ui.painter(),
                pos2(rect.left() + 22.0, rect.top() + 27.5),
                Align2::LEFT_CENTER,
                "Image Format",
                24.0,
                Weight::Bold,
                ink,
            );
            let group = Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.top() + 55.0),
                vec2(width - 20.0, 40.0 * n + n - 1.0),
            );
            ui.painter().rect_filled(group, 18.0, argb(ACTIONS_GROUP));
            for (i, format) in SHARE_FORMATS.into_iter().enumerate() {
                let row =
                    Rect::from_min_size(pos2(group.left(), group.top() + 41.0 * i as f32), vec2(group.width(), 40.0));
                if i > 0 {
                    let line = Rect::from_min_size(pos2(row.left(), row.top() - 1.0), vec2(row.width(), 1.0));
                    ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
                }
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    format.name(),
                    17.0,
                    Weight::Regular,
                    ink,
                );
                if ui.interact(row, ui.scope_id().with(("format", i)), Sense::click()).clicked() {
                    chosen = Some(format);
                }
            }
        });
        if close || chosen.is_some() {
            self.sharing = None;
        }
        chosen.map(|format| Asked::Share(list, format))
    }

    /// The box a cell's thumbnail fits in: the cell's width less 40, its height less 82, 39 down.
    fn thumb_box(cell: Rect) -> Rect {
        let (w, h) = (cell.width() - 40.0, cell.height() - 82.0);
        Rect::from_min_size(pos2(cell.center().x - w / 2.0, cell.top() + 39.0), vec2(w, h))
    }

    /// The artwork's picture as large as fits in `frame`, at its bottom middle.
    fn fitted(frame: Rect, a: &GalleryArtwork) -> Rect {
        let ratio = if a.width > 0 && a.height > 0 { a.width as f32 / a.height as f32 } else { 0.75 };
        let size = if ratio <= frame.width() / frame.height() {
            vec2(frame.height() * ratio, frame.height())
        } else {
            vec2(frame.width(), frame.width() / ratio)
        };
        Rect::from_min_size(pos2(frame.center().x - size.x / 2.0, frame.bottom() - size.y), size)
    }

    fn texture(&mut self, ctx: &egui::Context, a: &GalleryArtwork) -> Option<egui::TextureId> {
        let fresh = self.thumbs.get(&a.dir).is_none_or(|(when, _)| *when != a.modified_ms);
        if fresh {
            let path = std::path::Path::new(&a.dir).join("thumbnail.png");
            let texture = crate::files::read_png(&path).ok().map(|image| {
                let size = [image.width as usize, image.height as usize];
                let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
                ctx.load_texture(format!("thumbnail {}", a.dir), pixels, egui::TextureOptions::LINEAR)
            });
            self.thumbs.insert(a.dir.clone(), (a.modified_ms, texture));
        }
        self.thumbs.get(&a.dir).and_then(|(_, t)| t.as_ref().map(TextureHandle::id))
    }

    /// An artwork's thumbnail in `rect`, turned `turn` degrees: white under its picture, rounded
    /// 8 (but turned), with a soft shadow.
    fn thumbnail(&mut self, ui: &Ui, rect: Rect, a: &GalleryArtwork, turn: f32) {
        let texture = self.texture(ui.ctx(), a);
        let painter = ui.painter();
        if turn == 0.0 {
            let shadow =
                egui::epaint::Shadow { offset: [0, 0], blur: 20, spread: 0, color: Color32::from_black_alpha(0x80) };
            painter.add(shadow.as_shape(rect, 8));
            painter.rect_filled(rect, 8, Color32::WHITE);
            if let Some(t) = texture {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.add(egui::epaint::RectShape::filled(rect, 8, Color32::WHITE).with_texture(t, uv));
            }
            return;
        }
        let (sin, cos) = turn.to_radians().sin_cos();
        let c = rect.center();
        let turned = |p: Pos2| {
            let d = p - c;
            c + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
        };
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].map(turned);
        let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
        let quad = |texture: egui::TextureId| {
            let mut mesh = egui::Mesh::with_texture(texture);
            for (p, uv) in corners.iter().zip(uvs) {
                mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: Color32::WHITE });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            mesh
        };
        painter.add(egui::Shape::convex_polygon(
            corners.to_vec(),
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_black_alpha(0x30)),
        ));
        if let Some(t) = texture {
            painter.add(egui::Shape::mesh(quad(t)));
        }
    }

    /// Draws an artwork's cell; returns where its name is.
    fn artwork_cell(&mut self, ui: &Ui, cell: Rect, a: &GalleryArtwork) -> Rect {
        let frame = Self::thumb_box(cell);
        let picture = Self::fitted(frame, a);
        self.thumbnail(ui, picture, a, 0.0);
        let chosen = self.picked.contains(&a.dir);
        if self.selecting {
            // a circle at its top right: filled blue with a tick when picked, else a thin light ring
            let mark = pos2(picture.right() - 12.0 - 7.0, picture.top() + 12.0 + 7.0);
            if chosen {
                ui.painter().circle_filled(mark, 7.0, argb(PICKED));
            } else {
                ui.painter().circle_stroke(mark, 6.5, Stroke::new(1.0, argb(UNPICKED_RING)));
            }
        }
        if chosen {
            tick(ui.painter(), pos2(picture.right() - 19.0, picture.top() + 19.0));
        }
        let ink = if self.selecting && chosen { Color32::WHITE } else { argb(TEXT) };
        let name = one_line(ui, &a.name, 13.0, Weight::SemiBold, ink, frame.width());
        let at = pos2(cell.center().x - name.size().x / 2.0, frame.bottom() + 12.0);
        ui.painter().galley(at, name.clone(), Color32::WHITE);
        let size = format!("{} \u{d7} {}px", a.width, a.height);
        text(
            ui.painter(),
            pos2(cell.center().x, at.y + name.size().y),
            Align2::CENTER_TOP,
            &size,
            10.0,
            Weight::Regular,
            argb(SIZE),
        );
        Rect::from_min_size(at, name.size())
    }

    fn stack_cell(&mut self, ui: &Ui, cell: Rect, name: &str, artworks: &[GalleryArtwork]) {
        let Some(top) = artworks.first() else { return };
        let frame = Self::thumb_box(cell);
        let picture = Self::fitted(frame, top);
        // the top artwork again on two cards under it, a little turned (none for one)
        if artworks.len() > 1 {
            for turn in [5.0, -4.0] {
                self.thumbnail(ui, picture, top, turn);
            }
        }
        self.thumbnail(ui, picture, top, 0.0);
        // selecting, a stack can't be picked: its name dims
        let ink = if self.selecting { argb(SIZE) } else { argb(TEXT) };
        let label = one_line(ui, name, 13.0, Weight::SemiBold, ink, frame.width());
        let at = pos2(cell.center().x - label.size().x / 2.0, frame.bottom() + 12.0);
        ui.painter().galley(at, label.clone(), Color32::WHITE);
        let count = format!("{} artworks", artworks.len());
        text(
            ui.painter(),
            pos2(cell.center().x, at.y + label.size().y),
            Align2::CENTER_TOP,
            &count,
            10.0,
            Weight::Regular,
            argb(SIZE),
        );
    }

    /// The New canvas list hanging from +: the window's size first, then the presets (scrolling
    /// past 376), each with its colour profile and size. A tap off it closes it.
    fn new_canvas(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) -> Option<Asked> {
        let left = screen.right() - 2.0 - NEW_WIDTH;
        let arrow = screen.right() - 49.0 - left;
        let list_height = (45.0 + 45.0 * self.presets.len().saturating_sub(1) as f32).min(PRESETS_MOST);
        let height = panel::ARROW_HEIGHT + NEW_HEADER + 38.0 + 9.5 + list_height + 30.0;
        let rect = Rect::from_min_size(pos2(left, screen.top() + 56.5), vec2(NEW_WIDTH, height));
        let mut asked = None;
        let mut close = false;
        egui::Area::new(Id::unique("new canvas")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.interact(rect, ui.scope_id().with("panel"), Sense::click_and_drag());
            let body = panel::solid_frame(ui, look, "new canvas", rect, arrow, false, 32.0, LIGHT_PANEL);
            let ink = argb(LIGHT_TEXT);
            text(
                ui.painter(),
                pos2(body.left() + 23.0, body.top() + 21.0),
                Align2::LEFT_TOP,
                "New canvas",
                26.0,
                Weight::Bold,
                ink,
            );
            // the button that makes a custom canvas: a dark card with a plus
            let custom = Rect::from_min_size(pos2(body.right() - 13.0 - 44.0, body.top() + 18.0), vec2(44.0, 34.0));
            let card = Rect::from_center_size(custom.center(), vec2(27.0, 16.0));
            ui.painter().rect_filled(card, 4.0, ink);
            let plus = pos2(card.left() + 8.0, card.center().y);
            let stroke = Stroke::new(1.6, argb(LIGHT_PANEL));
            ui.painter().line_segment([plus - vec2(3.6, 0.0), plus + vec2(3.6, 0.0)], stroke);
            ui.painter().line_segment([plus - vec2(0.0, 3.6), plus + vec2(0.0, 3.6)], stroke);
            if ui.interact(custom, ui.scope_id().with("custom canvas"), Sense::click()).clicked() {
                self.custom = Some(crate::custom_canvas::CustomCanvas::new(None));
            }
            // the screen's size, a canvas in Display P3
            let (sw, sh) = self.screen_px;
            let row =
                Rect::from_min_size(pos2(body.left() + 9.0, body.top() + NEW_HEADER), vec2(NEW_WIDTH - 18.0, 38.0));
            ui.painter().rect_filled(row, 19.0, argb(LIGHT_ROW_STRONG));
            preset_row(ui, row, "Screen Size", &format!("{sw} \u{d7} {sh}px"), P3);
            if ui.interact(row, ui.scope_id().with("screen size"), Sense::click()).clicked() {
                asked = Some(Asked::ScreenSize(self.folder()));
            }
            let list = Rect::from_min_size(pos2(row.left(), row.bottom() + 9.5), vec2(row.width(), list_height));
            ui.painter().rect_filled(list, 24.0, argb(LIGHT_ROW));
            let presets = self.presets.clone();
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
                ui.set_clip_rect(list);
                ScrollArea::vertical().id_salt("presets").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (i, p) in presets.iter().enumerate() {
                        let (line, response) =
                            ui.allocate_exact_size(vec2(list.width(), 45.0), Sense::click_and_drag());
                        // slid left under a drag, it shows Edit and Delete behind it
                        let x = match self.preset_drag {
                            Some((d, x)) if d == i => x,
                            _ if self.preset_open == Some(i) => -PRESET_REVEAL,
                            _ => 0.0,
                        };
                        if response.drag_started() {
                            self.preset_drag = Some((i, x));
                        }
                        if let Some((d, x)) = self.preset_drag.as_mut()
                            && *d == i
                        {
                            *x = (*x + response.drag_delta().x).clamp(-PRESET_REVEAL, 0.0);
                        }
                        if response.drag_stopped() {
                            self.preset_open = (x < -PRESET_REVEAL / 2.0).then_some(i);
                            self.preset_drag = None;
                        }
                        if x < 0.0 {
                            ui.painter().rect_filled(line, 0, argb(PRESET_BEHIND));
                            let delete = Rect::from_min_size(
                                pos2(line.right() - 13.0 - 69.0, line.center().y - 16.5),
                                vec2(69.0, 33.0),
                            );
                            let edit = delete.translate(vec2(-72.0, 0.0));
                            for (button, label, fill, ink) in [
                                (edit, "Edit", PRESET_EDIT, argb(PRESET_EDIT_INK)),
                                (delete, "Delete", PRESET_DELETE, Color32::WHITE),
                            ] {
                                ui.painter().rect_filled(button, 7.0, argb(fill));
                                text(
                                    ui.painter(),
                                    button.center(),
                                    Align2::CENTER_CENTER,
                                    label,
                                    15.0,
                                    Weight::SemiBold,
                                    ink,
                                );
                                if ui
                                    .interact(button, ui.scope_id().with(("preset", label, i)), Sense::click())
                                    .clicked()
                                {
                                    self.preset_open = None;
                                    if label == "Edit" {
                                        self.custom =
                                            Some(crate::custom_canvas::CustomCanvas::new(Some((i, p.clone()))));
                                    } else if i < self.presets.len() {
                                        self.presets.remove(i);
                                        self.keep_presets();
                                    }
                                }
                            }
                        }
                        let shown = line.translate(vec2(x, 0.0));
                        if x < 0.0 {
                            ui.painter().rect_filled(shown, 0, argb(LIGHT_ROW));
                        }
                        if i > 0 {
                            let edge = Rect::from_min_size(
                                pos2(shown.left() + 16.0, shown.top()),
                                vec2(shown.width() - 16.0, 1.0),
                            );
                            ui.painter().rect_filled(edge, 0, argb(LIGHT_SEPARATOR));
                        }
                        preset_row(ui, shown, &p.name, &preset_label(p.clone()), &p.profile);
                        // a tap picks it, or puts away a row swiped open
                        if response.clicked() {
                            if self.preset_open.is_some() {
                                self.preset_open = None;
                            } else {
                                asked = Some(Asked::New(p.clone(), self.folder()));
                            }
                        }
                    }
                });
            });
        });
        if asked.is_some() || close {
            self.new_open = false;
        }
        asked
    }
}

/// The folder of what a gallery entry shows.
fn entry_dir(e: &GalleryEntry) -> &str {
    match e {
        GalleryEntry::Artwork { artwork } => &artwork.dir,
        GalleryEntry::Stack { dir, .. } => dir,
    }
}

/// A white tick centred at `c`, in a picked artwork's mark.
fn tick(painter: &egui::Painter, c: Pos2) {
    let stroke = Stroke::new(1.5, Color32::WHITE);
    painter.line_segment([c + vec2(-3.0, 0.0), c + vec2(-1.0, 2.5)], stroke);
    painter.line_segment([c + vec2(-1.0, 2.5), c + vec2(3.5, -2.5)], stroke);
}

/// A preset's row: its name, then chips for its colour profile and size.
fn preset_row(ui: &Ui, row: Rect, name: &str, size: &str, profile: &str) {
    let ink = argb(LIGHT_TEXT);
    let mut x = row.right() - 16.0;
    for label in [size.to_owned(), profile_chip(profile.to_owned())] {
        let galley = one_line(ui, &label, 13.0, Weight::Regular, ink, f32::INFINITY);
        let chip =
            Rect::from_min_max(pos2(x - galley.size().x - 16.0, row.center().y - 9.0), pos2(x, row.center().y + 9.0));
        ui.painter().rect_filled(chip, 9.0, argb(LIGHT_CHIP));
        ui.painter().galley(pos2(chip.left() + 8.0, chip.center().y - galley.size().y / 2.0), galley, ink);
        x = chip.left() - 4.0;
    }
    let galley = one_line(ui, name, 17.0, Weight::Regular, ink, x - 8.0 - (row.left() + 17.0));
    ui.painter().galley(pos2(row.left() + 17.0, row.center().y - galley.size().y / 2.0), galley, ink);
}

/// `text` on one line at most `max_width` wide, ending in "…" when cut.
fn one_line(ui: &Ui, text: &str, size: f32, weight: Weight, colour: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let format = TextFormat { font_id: weight.font(size), color: colour, ..Default::default() };
    job.append(text, 0.0, format);
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    ui.painter().layout_job(job)
}

fn presets_path() -> std::path::PathBuf {
    crate::files::app_dir().join("presets.json")
}
