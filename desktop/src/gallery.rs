//! The gallery, as the Android app's (ui/GalleryScreen.kt): every artwork as a thumbnail with its
//! name and size, a stack as its top artwork over two turned cards; over them the app's name (in a
//! stack, the way back and its name), Select, Import, Photo and +, which opens the New canvas list
//! (the window's size, then the presets). Select picks artworks to stack, preview, duplicate or
//! delete. The artworks are the core's store (gallery.rs), kept in the app's folder; the presets
//! are the core's too, kept as the Android app keeps them.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, TextureHandle, Ui, UiBuilder, Vec2, pos2, vec2,
};
use procraft_core::{
    ArtworkStore, CanvasPreset, GalleryArtwork, GalleryEntry, P3, preset_label, presets_from_json, profile_chip,
};

use crate::animated_export::Animated;
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};
use crate::panel;

pub const BACKGROUND: Themed = Themed(0xFF212121, 0xFFF0F0F0);
/// A thumbnail's shadow, dropped below it, and the rim round it.
const THUMB_SHADOW: Themed = Themed(0x45000000, 0x1D000000);
const THUMB_RIM: Themed = Themed(0xFF424242, 0xFFBEBEBE);
const TITLE: Themed = Themed(0xFFF0F0F0, 0xFF3C3C3C);
const TEXT: Themed = Themed(0xFFAAAAAA, 0xFF595959);
const SIZE: Themed = Themed(0xFF666666, 0xFF6A6A6A);
const CAPSULE: Themed = Themed(0xFF1C1C1C, 0xF0F7F7F7);
const CAPSULE_EDGE: Themed = Themed(0xFF393939, 0x0F000000);
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
const POPOVER: Themed = Themed(0xFF212121, 0xFFE6E6E6);
const ACTIONS_GROUP: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const ACTIONS_SEPARATOR: Themed = Themed(0x06FFFFFF, 0xFFE2E2E2);
const ACTIONS_DISABLED: Themed = Themed(0xFF5E5E5E, 0xFFABABAB);
const PANEL_TITLE: Themed = Themed(0xFFC7C7C7, 0xFF393939);
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
/// About's button: 520 under the mark's top, 220 x 38.
const ABOUT_BUTTON: (f32, f32) = (220.0, 38.0);
const ABOUT_BUTTON_DOWN: f32 = 520.0;
const ABOUT_BUTTON_FILL: u32 = 0xFF1A1A1A;
const ABOUT_BUTTON_INK: u32 = 0xFFC4C4C4;
const RENAME_FRAME: (f32, f32) = (281.0, 280.0);
const RENAME_TOP: f32 = 20.0;
const RENAME_MARGIN: f32 = 12.0;
const RENAME_GAP: f32 = 20.0;
const RENAME_FIELD: (f32, f32) = (262.0, 38.0);
const RENAME_FIELD_FILL: u32 = 0x14FFFFFF;
/// Renaming's picture rising from its cell, and its fading away when done (seconds).
const RENAME_RISE_S: f32 = 0.3;
const RENAME_FADE_S: f32 = 0.15;
/// A swiped artwork's actions: three pills over its thumbnail, which fades back.
const SWIPE_PILL: (f32, f32) = (94.0, 56.0);
const SWIPE_PILL_FILL: Themed = Themed(0xDB1C1C1C, 0xFFF5F5F5);
const SWIPE_PILL_INK: Themed = Themed(0xFFA9A9A9, 0xFF575757);
const SWIPE_DELETE_FILL: u32 = 0xFFFE604D;
const SWIPED_FADE: f32 = 0.64;
/// A swiped artwork's actions growing in or going (seconds), on a spring.
const SWIPE_S: f32 = 0.3;

fn swipe_ease(t: f32) -> f32 {
    crate::look::spring(SWIPE_S, 0.75, 0.0, t)
}
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
    /// A new artwork of a plugin's kind (`<id>.<key>`), the screen's size, in this folder.
    PluginKind(String, Option<String>),
    Import(Option<String>),
    Photo(Option<String>),
    /// These artworks written as this format, where the user picks.
    Share(Vec<GalleryArtwork>, crate::files::Format),
    /// These artworks written as PDFs keeping their pictures at this JPEG quality.
    SharePdf(Vec<GalleryArtwork>, u8),
    /// The artwork opened to be shared as this animation, the gallery back after.
    ShareAnimated(GalleryArtwork, Animated),
    /// The artwork lifted and dragged out of the window: handed to the system as a file.
    DragOut(GalleryArtwork),
}

/// Share's formats (`size`, arrow left out) beside a swiped artwork's Share `pill`, pointing at it:
/// on the side with more room (the other if that one is too narrow), kept 2 inside `screen`.
/// Where it goes (arrow in), its arrow's tip from its top, and whether it is right of the pill.
fn formats_beside(pill: Rect, screen: Rect, size: egui::Vec2) -> (Rect, f32, bool) {
    let (width, height) = (size.x, size.y);
    let top = (pill.center().y - height / 2.0)
        .clamp(screen.top() + 2.0, (screen.bottom() - 2.0 - height).max(screen.top() + 2.0));
    let ends = panel::POPOVER_CORNER + panel::ARROW_HEIGHT;
    let arrow_y = (pill.center().y - top).clamp(ends, height - ends);
    let total = width + panel::ARROW_HEIGHT;
    let room_right = screen.right() - 2.0 - pill.right() - 2.0;
    let room_left = pill.left() - 2.0 - (screen.left() + 2.0);
    let right = if room_right >= room_left {
        room_right >= total || room_left < total
    } else {
        room_left < total && room_right >= total
    };
    let left = if right { pill.right() + 2.0 } else { (pill.left() - 2.0 - total).max(screen.left() + 2.0) };
    (Rect::from_min_size(pos2(left, top), vec2(total, height)), arrow_y, right)
}

/// A row of Share's formats: a still format, or an animated one (the artwork opened to make it).
#[derive(Clone, Copy)]
enum ShareRow {
    Still(crate::files::Format),
    Animated(Animated),
}

impl ShareRow {
    fn name(self) -> &'static str {
        match self {
            ShareRow::Still(format) => format.name(),
            ShareRow::Animated(format) => format.title(),
        }
    }
}

/// Share's formats, as their list shows them, the animated ones last.
const SHARE_ROWS: [ShareRow; 11] = [
    ShareRow::Still(crate::files::Format::Procraft),
    ShareRow::Still(crate::files::Format::Package),
    ShareRow::Still(crate::files::Format::Psd),
    ShareRow::Still(crate::files::Format::Pdf),
    ShareRow::Still(crate::files::Format::Jpeg),
    ShareRow::Still(crate::files::Format::Png),
    ShareRow::Still(crate::files::Format::Tiff),
    ShareRow::Animated(Animated::Gif),
    ShareRow::Animated(Animated::Png),
    ShareRow::Animated(Animated::Mp4),
    ShareRow::Animated(Animated::Hevc),
];
const FORMATS_WIDTH: f32 = 260.0;

pub struct Gallery {
    pub store: Arc<ArtworkStore>,
    entries: Vec<GalleryEntry>,
    /// The stack open: its folder and name.
    stack: Option<(String, String)>,
    /// Each artwork's thumbnail (by folder), and when the artwork was changed when it was read.
    thumbs: HashMap<String, (i64, Option<TextureHandle>)>,
    /// Thumbnails being read on a thread of their own (a gallery of many would hold up the
    /// window), and that thread.
    reading: std::collections::HashSet<(String, i64)>,
    reader: Option<ThumbnailReader>,
    /// Artworks being duplicated on threads of their own (a big one's files take a while): the
    /// gallery is read again as each batch is done.
    duplicating: Vec<std::sync::mpsc::Receiver<()>>,
    new_open: bool,
    presets: Vec<CanvasPreset>,
    /// The kinds of artwork the plugins on make: the New canvas list's last rows.
    pub kinds: Vec<procraft_core::PluginKind>,
    /// The screen's size, pixels: Screen Size's canvas.
    pub screen_px: (u32, u32),
    /// Select: on, the artworks picked (by folder), More's list open, the picked artworks shown
    /// one at a time (and which), and the alert asking before they are deleted.
    selecting: bool,
    picked: Vec<String>,
    more_open: bool,
    preview: Option<(Vec<GalleryArtwork>, usize, bool, f32)>,
    /// The animation of the artwork shown in Preview, by its folder.
    played: Option<Played>,
    /// An artwork two fingers are spreading open into Preview.
    spreading: Option<Spread>,
    alert: crate::alert::Alert,
    /// Share's PDF asks how well it keeps its pictures first (for these artworks).
    pdf_alert: crate::alert::Alert,
    pdf_for: Option<Vec<GalleryArtwork>>,
    /// An artwork lifted and dragged out past the window's edge.
    dragged_out: Option<GalleryArtwork>,
    /// The artwork being renamed, its name as typed, and whether its field has had the focus.
    renaming: Option<(GalleryArtwork, String, bool)>,
    /// The stack being renamed instead (its folder), shown as its top artwork under its name.
    renaming_stack: Option<String>,
    /// When the renaming began (its picture rising from its cell), and one just done, fading
    /// away (its name as kept, since when).
    rename_since: Option<Instant>,
    rename_fading: Option<(GalleryArtwork, String, Instant)>,
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
    /// Where each artwork's picture (by folder) was last drawn: what the gallery zooms into and
    /// out of as an artwork opens and closes.
    pictures: HashMap<String, Rect>,
    /// Pictures shrinking under a press, growing as they lift, or going back (by folder).
    lifts: HashMap<String, Lift>,
    /// A stack opening, its artworks flying out of its cell (where it was, since when); or left,
    /// its artworks flying back into it over the gallery coming back.
    opening_stack: Option<(Rect, Instant)>,
    leaving_stack: Option<LeavingStack>,
    /// Where each stack's cell was last drawn (by folder), and the cells drawn last frame: what
    /// flies back into a stack as it is left.
    stack_cells: HashMap<String, Rect>,
    drawn_cells: Vec<(GalleryEntry, Rect)>,
}

/// A stack just left: its folder and name, its artworks and where each was, and since when.
struct LeavingStack {
    dir: String,
    name: String,
    cells: Vec<(GalleryEntry, Rect)>,
    since: Instant,
}

/// A stack's artworks flying out of its cell as it opens, and back into it (as the gallery grows
/// back from 0.8 of its size, fading in) as it is left (seconds, on springs); leaving, its name
/// fading out and the gallery's in.
const STACK_OPEN_S: f32 = 0.45;

/// Two fingers spread on an artwork: let go spread this much or more, it opens in Preview; it
/// shrinks no further than the least; let go, it settles in this long, seconds.
const SPREAD_OPEN: f32 = 1.35;
const SPREAD_LEAST: f32 = 0.6;
const SPREAD_SETTLE_S: f32 = 0.3;

/// An artwork two fingers are spreading open: its picture where it showed, how far the fingers
/// have spread and moved it, and once they let go, since when, from where, and whether it opens.
struct Spread {
    artwork: GalleryArtwork,
    from: Rect,
    scale: f32,
    moved: Vec2,
    let_go: Option<(Instant, Rect, bool)>,
}

impl Spread {
    /// Where the fingers have it: grown about its middle and moved with them.
    fn following(&self) -> Rect {
        Rect::from_center_size(self.from.center() + self.moved, self.from.size() * self.scale)
    }
}
/// Select's marks fading in and out (seconds).
const SELECT_FADE_S: f32 = 0.15;
const STACK_LEAVE_S: f32 = 0.3;
const STACK_GROW_FROM: f32 = 0.8;
const NAME_OUT_S: f32 = 0.07;
const TITLE_IN_S: f32 = 0.11;

/// A picture's size changing: from and to (times its size), since when, and how long (seconds).
struct Lift {
    from: f32,
    to: f32,
    since: Instant,
    secs: f32,
}

impl Lift {
    fn now(&self) -> f32 {
        let t = if self.secs > 0.0 { self.since.elapsed().as_secs_f32() / self.secs } else { 1.0 };
        self.from + (self.to - self.from) * crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
    }

    fn moving(&self) -> bool {
        self.since.elapsed().as_secs_f32() < self.secs
    }
}

/// Pressed, a picture waits a moment (so a scroll starting on it leaves it be), then shrinks;
/// held, it grows as it lifts; let go, it goes back (times its size, seconds).
const PRESS_DELAY_S: f64 = 0.1;
const PRESSED: (f32, f32) = (0.87, 0.15);
const LIFTED: (f32, f32) = (1.07, 0.25);
const DROPPED: (f32, f32) = (1.0, 0.25);

impl Gallery {
    pub fn new() -> Gallery {
        let store = ArtworkStore::open(crate::files::app_dir().join("artworks").to_string_lossy().into_owned());
        let presets = presets_from_json(std::fs::read_to_string(presets_path()).ok());
        let mut gallery = Gallery {
            store,
            entries: Vec::new(),
            stack: None,
            thumbs: HashMap::new(),
            reading: Default::default(),
            reader: None,
            duplicating: Vec::new(),
            new_open: false,
            presets,
            kinds: Vec::new(),
            screen_px: (1920, 1080),
            selecting: false,
            picked: Vec::new(),
            more_open: false,
            preview: None,
            played: None,
            spreading: None,
            alert: crate::alert::Alert::default(),
            pdf_alert: crate::alert::Alert::default(),
            pdf_for: None,
            dragged_out: None,
            renaming: None,
            renaming_stack: None,
            rename_since: None,
            rename_fading: None,
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
            pictures: HashMap::new(),
            lifts: HashMap::new(),
            opening_stack: None,
            leaving_stack: None,
            stack_cells: HashMap::new(),
            drawn_cells: Vec::new(),
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
        if let Err(e) =
            crate::files::write_whole(&presets_path(), procraft_core::presets_json(self.presets.clone()).as_bytes())
        {
            log::warn!("keeping the presets: {e}");
        }
    }

    /// The folder new artworks go in: the stack open's, or the gallery's.
    pub fn folder(&self) -> Option<String> {
        self.stack.as_ref().map(|s| s.0.clone())
    }

    /// How big the picture of `dir` is now (times its size), going to `to` in `secs` when that
    /// is new.
    fn lift(&mut self, ctx: &egui::Context, dir: &str, (to, secs): (f32, f32)) -> f32 {
        let lift =
            self.lifts.entry(dir.to_owned()).or_insert(Lift { from: 1.0, to: 1.0, since: Instant::now(), secs: 0.0 });
        if lift.to != to {
            *lift = Lift { from: lift.now(), to, since: Instant::now(), secs };
        }
        let now = lift.now();
        if lift.moving() {
            ctx.request_repaint();
        } else if to == 1.0 {
            self.lifts.remove(dir);
        }
        now
    }

    /// Where the artwork in folder `dir` was last drawn, if it was.
    pub fn picture_of(&self, dir: &str) -> Option<Rect> {
        self.pictures.get(dir).copied()
    }

    /// The gallery's own page (its header and grid), as the window's background.
    pub fn layer() -> egui::LayerId {
        egui::LayerId::new(Order::Background, Id::unique("gallery"))
    }

    /// The gallery's page over the canvas opening, moved by `transform` and shown `alpha`; nothing
    /// in it is touched.
    pub fn zooming(&mut self, ctx: &egui::Context, look: &mut Look, transform: egui::emath::TSTransform, alpha: f32) {
        ctx.set_transform_layer(egui::LayerId::new(Order::Foreground, Id::unique("gallery")), transform);
        self.page(ctx, look, Order::Foreground, alpha, true);
    }

    /// The gallery's page (its header and grid) over the whole window, `order` deep, shown
    /// `alpha`; `covered`, nothing in it is touched. What a tap in it asked for.
    fn page(&mut self, ctx: &egui::Context, look: &mut Look, order: Order, alpha: f32, covered: bool) -> Option<Asked> {
        let screen = ctx.content_rect();
        // shown at once, not faded in as a new area would be
        egui::Area::new(Id::unique("gallery"))
            .fixed_pos(screen.min)
            .order(order)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                ui.allocate_rect(screen, Sense::hover());
                ui.painter().rect_filled(screen, 0, argb(BACKGROUND));
                let tapped = self.header(ui, look, screen);
                let grid = Rect::from_min_max(pos2(screen.left(), screen.top() + HEADER), screen.max);
                let asked = if self.entries.is_empty() {
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
                    // a stack just left: the gallery grows back into place under its artworks
                    match self.leaving_stack.as_ref().map(|l| l.since.elapsed().as_secs_f32() / STACK_LEAVE_S) {
                        Some(t) if t < 1.0 => {
                            let e = crate::look::spring(STACK_LEAVE_S, 0.8, 0.0, t);
                            let s = STACK_GROW_FROM + (1.0 - STACK_GROW_FROM) * e;
                            let c = grid.center().to_vec2();
                            let grow = egui::emath::TSTransform::new(c * (1.0 - s), s);
                            ui.with_visual_transform(grow, |ui| {
                                ui.multiply_opacity(e.clamp(0.0, 1.0));
                                self.grid(ui, grid)
                            })
                            .inner
                            .or(tapped)
                        }
                        _ => self.grid(ui, grid).or(tapped),
                    }
                };
                if covered {
                    // over everything in it: taken before any of it sees a press
                    ui.allocate_rect(screen, Sense::click_and_drag());
                    return None;
                }
                asked
            })
            .inner
    }

    /// Shows the gallery over the whole window; returns what a tap asked for.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, colours: &crate::colours::Colours) -> Option<Asked> {
        self.thumbnails_read(ctx);
        self.duplicated(ctx);
        let screen = ctx.content_rect();
        let mut asked = self.page(ctx, look, Order::Background, 1.0, false);
        self.stack_flights(ctx, look);
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
        if self.spreading.is_some() || ctx.input(|i| i.multi_touch().is_some()) {
            self.spread(ctx, screen);
        }
        if self.preview.is_some() {
            self.preview(ctx, look, screen);
        }
        if self.renaming.is_some() {
            self.rename(ctx, screen);
        }
        self.rename_fade(ctx, screen);
        if self.sharing.is_some() {
            asked = self.formats(ctx, look, screen).or(asked);
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
        if let Some(answer) = self.pdf_alert.show(ctx, look)
            && let Some(list) = self.pdf_for.take()
            && let Some(quality) = crate::alert::pdf_jpeg(answer)
        {
            asked = Some(Asked::SharePdf(list, quality));
        }
        if let Some(artwork) = self.dragged_out.take() {
            asked = Some(Asked::DragOut(artwork));
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
                self.duplicate(self.picked_artworks().into_iter().map(|a| a.dir).collect());
                self.end_selecting();
            }
            SelectAction::Delete => {
                let n = self.picked.len();
                let title = if n == 1 { "Delete 1 artwork?".to_owned() } else { format!("Delete {n} artworks?") };
                self.alert.open(&title, "This can't be undone.", "Delete", Some(&icons::TRASH));
            }
            SelectAction::More => self.more_open = !self.more_open,
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
    /// The artwork done renaming fading away (0.15 s), as it was left.
    fn rename_fade(&mut self, ctx: &egui::Context, screen: Rect) {
        let Some((a, name, since)) = self.rename_fading.clone() else { return };
        let t = since.elapsed().as_secs_f32() / RENAME_FADE_S;
        if t >= 1.0 {
            self.rename_fading = None;
            return;
        }
        ctx.request_repaint();
        let alpha = 1.0 - crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t);
        egui::Area::new(Id::unique("renamed artwork")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            let (picture, field) = Self::rename_places(&a, screen);
            ui.painter().rect_filled(screen, 0, argb(RENAME_VEIL));
            self.thumbnail(ui, picture, &a, 0.0);
            ui.painter().rect_filled(field, 10.0, argb(RENAME_FIELD_FILL));
            text(ui.painter(), field.center(), Align2::CENTER_CENTER, &name, 17.0, Weight::Regular, Color32::WHITE);
        });
    }

    /// Where renaming shows an artwork's picture (fitted in its frame, centred across the top) and
    /// the field under it.
    fn rename_places(a: &GalleryArtwork, screen: Rect) -> (Rect, Rect) {
        let (fw, fh) = (RENAME_FRAME.0 - RENAME_MARGIN * 2.0, RENAME_FRAME.1 - RENAME_MARGIN * 2.0);
        let ratio = if a.width > 0 && a.height > 0 { a.width as f32 / a.height as f32 } else { 0.75 };
        let (aw, ah) = if ratio > fw / fh { (fw, fw / ratio) } else { (fh * ratio, fh) };
        let top = screen.top() + RENAME_TOP + (RENAME_FRAME.1 - ah) / 2.0;
        let picture = Rect::from_min_size(pos2(screen.center().x - aw / 2.0, top), vec2(aw, ah));
        let field_top = screen.top() + RENAME_TOP + RENAME_FRAME.1 + RENAME_GAP;
        let field = Rect::from_min_size(
            pos2(screen.center().x - RENAME_FIELD.0 / 2.0, field_top),
            vec2(RENAME_FIELD.0, RENAME_FIELD.1),
        );
        (picture, field)
    }

    fn rename(&mut self, ctx: &egui::Context, screen: Rect) {
        let Some((a, mut name, mut focused)) = self.renaming.take() else { return };
        let mut done = false;
        // the picture rises from its cell to the top (0.3 s), the field showing as it comes
        let since = *self.rename_since.get_or_insert_with(Instant::now);
        let t = since.elapsed().as_secs_f32() / RENAME_RISE_S;
        let rise = crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t);
        if t < 1.0 {
            ctx.request_repaint();
        }
        egui::Area::new(Id::unique("rename artwork")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            done = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.painter().rect_filled(screen, 0, argb(RENAME_VEIL));
            let (picture, field) = Self::rename_places(&a, screen);
            let from = self.picture_of(&a.dir).unwrap_or(picture);
            let at = Rect::from_min_max(from.min.lerp(picture.min, rise), from.max.lerp(picture.max, rise));
            self.thumbnail(ui, at, &a, 0.0);
            ui.painter().rect_filled(field, 10.0, argb(RENAME_FIELD_FILL).gamma_multiply(rise));
            let inner = field.shrink2(vec2(12.0, 0.0));
            let mut output = egui::TextEdit::singleline(&mut name)
                .font(Weight::Regular.font(17.0))
                .text_color(Color32::WHITE)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .align(Align2::CENTER_CENTER)
                .desired_width(inner.width())
                .char_limit(256)
                .show(&mut {
                    let mut child = ui.new_child(
                        UiBuilder::new().max_rect(Rect::from_center_size(inner.center(), vec2(inner.width(), 22.0))),
                    );
                    child.multiply_opacity(rise);
                    child
                });
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
            self.rename_since = None;
            self.rename_fading = Some((a.clone(), kept.clone(), Instant::now()));
            match self.renaming_stack.take() {
                Some(stack) => self.store.rename_stack(stack, kept),
                None => self.store.rename(a.dir, kept),
            }
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
            let version = concat!("Version ", env!("PROCRAFT_VERSION"));
            text(
                ui.painter(),
                pos2(field.center().x, name.bottom() + 14.0),
                Align2::CENTER_TOP,
                version,
                13.0,
                Weight::Regular,
                argb(ABOUT_INK),
            );
            let at = pos2(field.center().x - ABOUT_BUTTON.0 / 2.0, field.top() + ABOUT_BUTTON_DOWN);
            let recovery = Rect::from_min_size(at, vec2(ABOUT_BUTTON.0, ABOUT_BUTTON.1));
            ui.painter().rect_filled(recovery, 8.0, argb(ABOUT_BUTTON_FILL));
            let ink = argb(ABOUT_BUTTON_INK);
            text(
                ui.painter(),
                recovery.center(),
                Align2::CENTER_CENTER,
                "Start Gallery Recovery",
                15.0,
                Weight::Regular,
                ink,
            );
            // nothing is looked through while a file is still being written
            let tapped = ui.interact(recovery, ui.scope_id().with("about recovery"), Sense::click()).clicked();
            if tapped && !crate::files::still_writing() {
                self.store.recover_lost_artworks();
                self.refresh();
                close = true;
            }
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
    /// Two fingers spread on an artwork (a touch screen's): its picture follows them out of its
    /// place as the gallery darkens; let go, it settles fitted over black and opens in Preview
    /// among the gallery's artworks or, spread too little, goes back into its place (as the
    /// tablet's).
    fn spread(&mut self, ctx: &egui::Context, screen: Rect) {
        let touch = ctx.input(|i| i.multi_touch());
        if self.spreading.is_none() {
            let Some(t) = touch.filter(|t| t.num_touches == 2) else { return };
            if self.selecting || self.preview.is_some() || self.lifted.is_some() {
                return;
            }
            let found = self.entries.iter().find_map(|e| match e {
                GalleryEntry::Artwork { artwork } => {
                    self.pictures.get(&artwork.dir).filter(|r| r.contains(t.start_pos)).map(|r| (artwork.clone(), *r))
                }
                _ => None,
            });
            let Some((artwork, from)) = found else { return };
            self.spreading = Some(Spread { artwork, from, scale: 1.0, moved: Vec2::ZERO, let_go: None });
        }
        let Some(s) = self.spreading.as_mut() else { return };
        match (touch, s.let_go) {
            (Some(t), None) => {
                s.scale = (s.scale * t.zoom_delta).max(SPREAD_LEAST);
                s.moved += t.translation_delta;
            }
            (None, None) => s.let_go = Some((Instant::now(), s.following(), s.scale >= SPREAD_OPEN)),
            _ => {}
        }
        // fitted to the screen, as Preview shows it
        let fitted = Self::fitted(screen, &s.artwork);
        let full = fitted.translate(vec2(0.0, screen.center().y - fitted.center().y));
        let (rect, settled) = match s.let_go {
            None => (s.following(), None),
            Some((since, held, opens)) => {
                let t = (since.elapsed().as_secs_f32() / SPREAD_SETTLE_S).min(1.0);
                let e = crate::look::cubic_bezier(0.0, 0.0, 0.58, 1.0, t);
                let to = if opens { full } else { s.from };
                (Rect::from_min_max(held.min.lerp(to.min, e), held.max.lerp(to.max, e)), (t >= 1.0).then_some(opens))
            }
        };
        let from = s.from;
        let artwork = s.artwork.clone();
        // how near the whole screen it is: the gallery darkens as far
        let near = ((rect.width() - from.width()) / (full.width() - from.width()).max(1.0)).clamp(0.0, 1.0);
        let texture = self.texture(&artwork);
        egui::Area::new(Id::unique("gallery spread")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.painter().rect_filled(screen, 0, Color32::BLACK.gamma_multiply(near));
            if let Some(t) = texture {
                egui::Image::new(egui::load::SizedTexture::new(t, rect.size()))
                    .corner_radius(8.0 * (1.0 - near))
                    .paint_at(ui, rect);
            }
        });
        ctx.request_repaint();
        if let Some(opens) = settled {
            self.spreading = None;
            if opens {
                let all: Vec<GalleryArtwork> = self
                    .entries
                    .iter()
                    .filter_map(|e| match e {
                        GalleryEntry::Artwork { artwork } => Some(artwork.clone()),
                        _ => None,
                    })
                    .collect();
                let index = all.iter().position(|a| a.dir == artwork.dir).unwrap_or(0);
                self.preview = Some((all, index, false, 0.0));
            }
        }
    }

    fn preview(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) {
        let Some((list, index, controls, moved)) = self.preview.take() else { return };
        let Some(a) = list.get(index).cloned() else { return };
        let (mut index, mut controls, mut moved, mut closed) = (index, controls, moved, false);
        egui::Area::new(Id::unique("preview")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, Color32::BLACK);
            self.play(ctx, &a);
            let frame = self.played.as_ref().and_then(Played::frame);
            if let Some(t) = frame.or_else(|| self.texture(&a)) {
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
        if closed {
            self.played = None;
        } else {
            self.preview = Some((list, index, controls, moved));
        }
    }

    /// Preview's artwork `a`, if it is animated: its frames read on a thread, then played as its
    /// settings say (its thumbnail shows until then).
    fn play(&mut self, ctx: &egui::Context, a: &GalleryArtwork) {
        if self.played.as_ref().is_none_or(|p| p.dir != a.dir) {
            self.played = Some(Played::start(a.dir.clone(), self.screen_px.0.max(self.screen_px.1)));
        }
        if let Some(p) = self.played.as_mut() {
            p.go_on(ctx);
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
                if let Some((dir, name)) = self.stack.take() {
                    let cells = std::mem::take(&mut self.drawn_cells);
                    self.leaving_stack = Some(LeavingStack { dir, name, cells, since: Instant::now() });
                }
                self.refresh();
            }
        } else {
            // a stack just left: its name fades out as the gallery's fades in
            let ease = |t: f32| crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t);
            let left = self.leaving_stack.as_ref().map(|l| (l.name.clone(), l.since.elapsed().as_secs_f32()));
            let title_in = left.as_ref().map_or(1.0, |(_, t)| ease(t / TITLE_IN_S));
            if let Some((name, t)) = &left
                && *t < NAME_OUT_S
            {
                let ink = argb(TITLE).gamma_multiply(1.0 - ease(t / NAME_OUT_S));
                let row = Rect::from_min_size(pos2(screen.left() + 20.0, top + 12.0), vec2(40.0, 41.0));
                look.icon(ui, &icons::CHEVRON_LEFT, row.center(), 28.0, ink);
                text(painter, pos2(row.right(), row.center().y), Align2::LEFT_CENTER, name, 36.0, Weight::Bold, ink);
            }
            let name = text(
                painter,
                pos2(screen.left() + 32.0, top + 11.0),
                Align2::LEFT_TOP,
                "ProCraft",
                36.0,
                Weight::Bold,
                argb(TITLE).gamma_multiply(title_in),
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
        let mut drawn = Vec::new();
        // a stack opening: its artworks fly out of its cell to their places
        let opening = self.opening_stack.and_then(|(from, since)| {
            let t = since.elapsed().as_secs_f32() / STACK_OPEN_S;
            (t < 1.0).then(|| (from, crate::look::spring(STACK_OPEN_S, 0.8, 0.0, t)))
        });
        if opening.is_some() {
            ui.ctx().request_repaint();
        } else {
            self.opening_stack = None;
        }
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
                    let cell = match opening {
                        Some((from, e)) => cell.translate((from.min - cell.min) * (1.0 - e)),
                        None => cell,
                    };
                    drawn.push((entry.clone(), cell));
                    if let GalleryEntry::Stack { dir, .. } = entry {
                        self.stack_cells.insert(dir.clone(), cell);
                    }
                    let response = ui.interact(cell, ui.scope_id().with(("gallery cell", i)), Sense::click_and_drag());
                    cells.push((i, cell));
                    // held still, it lifts; a lift let go where it was is no tap
                    if self.lifted.is_none()
                        && self.spreading.is_none()
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
                    // its picture shrinks under a press and grows as it lifts
                    let dir = entry_dir(entry);
                    let pressed = ui.input(|inp| inp.pointer.press_start_time().map_or(0.0, |t| inp.time - t));
                    let down = response.is_pointer_button_down_on() && !response.dragged();
                    let size = if self.lifted.as_ref().is_some_and(|(d, _)| d == dir) {
                        LIFTED
                    } else if down && pressed >= PRESS_DELAY_S {
                        PRESSED
                    } else {
                        if down {
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(PRESS_DELAY_S - pressed));
                        }
                        DROPPED
                    };
                    let scale = self.lift(ui.ctx(), dir, size);
                    match entry {
                        GalleryEntry::Artwork { artwork } => {
                            self.artwork_cell(ui, cell, artwork, scale);
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
                            // swiped open, it fades back and its actions grow in over it
                            let swiped = self.swiped.as_ref() == Some(&artwork.dir);
                            let id = ui.scope_id().with(("swipe open", &artwork.dir));
                            let open = ui.ctx().animate_bool_with_time_and_easing(id, swiped, SWIPE_S, swipe_ease);
                            if open > 0.0 {
                                self.swipe_actions(ui, cell, artwork, open, swiped);
                            }
                            // selecting, a tap picks it (or lets it go); a tap puts away actions
                            // showing
                            if tapped && self.swiped.is_some() {
                                self.swiped = None;
                            } else if tapped && on_name && !self.selecting {
                                self.renaming = Some((artwork.clone(), artwork.name.clone(), false));
                                self.renaming_stack = None;
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
                            // on its name, a tap renames it, as an artwork's does
                            let below = cell.top() + 39.0 + Self::thumb_box(cell).height() + 8.0;
                            let on_name = response.interact_pointer_pos().is_some_and(|p| p.y > below);
                            if tapped
                                && on_name
                                && !self.selecting
                                && let Some(top) = artworks.first()
                            {
                                let shown = GalleryArtwork { name: name.clone(), ..top.clone() };
                                self.renaming = Some((shown, name.clone(), false));
                                self.renaming_stack = Some(dir.clone());
                            } else if tapped && !self.selecting {
                                self.stack = Some((dir.clone(), name.clone()));
                                self.opening_stack = Some((cell, Instant::now()));
                                into_stack = true;
                            }
                        }
                    }
                }
            });
        });
        self.drag(ui, &entries, &cells);
        self.drawn_cells = drawn;
        if into_stack {
            self.refresh();
        }
        asked
    }

    /// A stack just left: its artworks flying back into its cell, over the gallery coming back.
    fn stack_flights(&mut self, ctx: &egui::Context, _look: &mut Look) {
        let Some(leaving) = self.leaving_stack.take() else { return };
        let t = leaving.since.elapsed().as_secs_f32() / STACK_LEAVE_S;
        if t >= 1.0 {
            return;
        }
        ctx.request_repaint();
        let e = crate::look::spring(STACK_LEAVE_S, 0.8, 0.0, t);
        let to = self.stack_cells.get(&leaving.dir).copied();
        let screen = ctx.content_rect();
        egui::Area::new(Id::unique("stack left"))
            .fixed_pos(screen.min)
            .order(Order::Foreground)
            .interactable(false)
            .fade_in(false)
            .show(ctx, |ui| {
                for (entry, cell) in &leaving.cells {
                    // to the stack's cell, its top left there (where it isn't known, they just go)
                    let at = match to {
                        Some(to) => cell.translate((to.min - cell.min) * e),
                        None => *cell,
                    };
                    let mut faded = ui.new_child(UiBuilder::new().max_rect(screen));
                    if to.is_none() {
                        faded.multiply_opacity(1.0 - e.clamp(0.0, 1.0));
                    }
                    match entry {
                        GalleryEntry::Artwork { artwork } => _ = self.artwork_cell(&faded, at, artwork, 1.0),
                        GalleryEntry::Stack { name, artworks, .. } => self.stack_cell(&faded, at, name, artworks),
                    }
                }
            });
        self.leaving_stack = Some(leaving);
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
        // an artwork taken past the window's edge leaves it, as a file for wherever it is let go
        // (the pointer outside, or gone as the window lost it)
        if let GalleryEntry::Artwork { artwork } = &entry
            && down
            && cfg!(windows)
            && pointer.is_none_or(|p| !ui.ctx().content_rect().contains(p))
        {
            self.dragged_out = Some(artwork.clone());
            self.lifted = None;
            return;
        }
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
            // grown as it lifts, about where it is held
            let k = self.lifts.get(&dir).map_or(LIFTED.0, Lift::now);
            let picture = Self::fitted(Self::thumb_box(cell), &a).translate(p - from);
            let picture = Rect::from_min_max(p + (picture.min - p) * k, p + (picture.max - p) * k);
            let texture = self.texture(&a);
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
    /// A swiped artwork's actions, `open` of the way in (they grow from a fifth of their size
    /// about their right end, fading in, as the artwork fades back); tappable once `shown`.
    fn swipe_actions(&mut self, ui: &Ui, cell: Rect, a: &GalleryArtwork, open: f32, shown: bool) {
        let alpha = open.clamp(0.0, 1.0);
        ui.painter().rect_filled(cell, 0, argb(BACKGROUND).gamma_multiply(SWIPED_FADE * alpha));
        let frame = Self::thumb_box(cell);
        let width = SWIPE_PILL.0 * 3.0 + 6.0;
        let (k, origin) = (0.2 + 0.8 * open, pos2(cell.center().x + width / 2.0, frame.center().y));
        let grown = |r: Rect| Rect::from_min_max(origin + (r.min - origin) * k, origin + (r.max - origin) * k);
        let mut x = cell.center().x - width / 2.0;
        for (label, delete) in [("Share", false), ("Duplicate", false), ("Delete", true)] {
            let pill =
                Rect::from_min_size(pos2(x, frame.center().y - SWIPE_PILL.1 / 2.0), vec2(SWIPE_PILL.0, SWIPE_PILL.1));
            x += SWIPE_PILL.0 + 3.0;
            let drawn = grown(pill);
            let fill = if delete { argb(SWIPE_DELETE_FILL) } else { argb(SWIPE_PILL_FILL) };
            ui.painter().rect_filled(drawn, 12.0 * k, fill.gamma_multiply(alpha));
            let ink = if delete { Color32::WHITE } else { argb(SWIPE_PILL_INK) };
            let words = (15.0 * k).max(1.0);
            text(
                ui.painter(),
                drawn.center(),
                Align2::CENTER_CENTER,
                label,
                words,
                Weight::SemiBold,
                ink.gamma_multiply(alpha),
            );
            if !shown {
                continue;
            }
            if ui.interact(pill, ui.scope_id().with(("swiped", label)), Sense::click()).clicked() {
                match label {
                    "Duplicate" => {
                        self.swiped = None;
                        self.duplicate(vec![a.dir.clone()]);
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
        }
    }

    /// Share's formats: beside a swiped artwork's Share, pointing at it, on the side with more
    /// room (the other if that one is too narrow), kept 2 inside the window; or under Select's
    /// Share, centred on it, kept 4 inside. A tap on one shares in it, a tap off closes.
    fn formats(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) -> Option<Asked> {
        let (list, from) = self.sharing.clone()?;
        let (width, n) = (FORMATS_WIDTH, SHARE_ROWS.len() as f32);
        let height = 55.0 + 40.0 * n + (n - 1.0) + 10.0;
        let (rect, arrow) = match from {
            Some(pill) => {
                let (rect, arrow_y, right) = formats_beside(pill, screen, vec2(width, height));
                (rect, Some((arrow_y, right)))
            }
            None => {
                let centre = self.share_button.center().x;
                let left = (centre - width / 2.0).clamp(screen.left() + 4.0, screen.right() - 4.0 - width);
                (Rect::from_min_size(pos2(left, screen.top() + 69.0), vec2(width, height)), None)
            }
        };
        let (mut chosen, mut close) = (None, false);
        egui::Area::new(Id::unique("share formats")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked();
            ui.interact(rect, ui.scope_id().with("panel"), Sense::click_and_drag());
            let rect = match arrow {
                // its arrow on its left when it is right of the pill
                Some((arrow_y, right)) => panel::solid_side_frame(
                    ui,
                    look,
                    "share formats",
                    rect,
                    arrow_y,
                    right,
                    panel::POPOVER_CORNER,
                    POPOVER,
                ),
                None => {
                    let shadow = egui::epaint::Shadow {
                        offset: [0, 0],
                        blur: 51,
                        spread: 0,
                        color: argb(panel::POPOVER_SHADOW),
                    };
                    ui.painter().add(shadow.as_shape(rect, panel::POPOVER_CORNER));
                    ui.painter().rect_filled(rect, panel::POPOVER_CORNER, argb(POPOVER));
                    rect
                }
            };
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
            for (i, format) in SHARE_ROWS.into_iter().enumerate() {
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
        match chosen? {
            // how well a PDF keeps its pictures, asked first
            ShareRow::Still(crate::files::Format::Pdf) => {
                self.pdf_for = Some(list);
                self.pdf_alert.ask(crate::alert::pdf_quality());
                None
            }
            ShareRow::Still(format) => Some(Asked::Share(list, format)),
            // an animation is made from the first of them
            ShareRow::Animated(format) => {
                let first = list.into_iter().next()?;
                self.end_selecting();
                Some(Asked::ShareAnimated(first, format))
            }
        }
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

    /// An artwork's thumbnail: read on the reader's thread the first time (and after it
    /// changes), the one before (or none) shown meanwhile.
    fn texture(&mut self, a: &GalleryArtwork) -> Option<egui::TextureId> {
        let fresh = self.thumbs.get(&a.dir).is_none_or(|(when, _)| *when != a.modified_ms);
        let asked = (a.dir.clone(), a.modified_ms);
        if fresh && !self.reading.contains(&asked) {
            let path = std::path::Path::new(&a.dir).join("thumbnail.png");
            let reader = self.reader.get_or_insert_with(ThumbnailReader::start);
            if reader.asks.send((asked.0.clone(), asked.1, path)).is_ok() {
                self.reading.insert(asked);
            }
        }
        self.thumbs.get(&a.dir).and_then(|(_, t)| t.as_ref().map(TextureHandle::id))
    }

    /// The artworks in `dirs` duplicated, each beside itself, on a thread of their own (one batch
    /// at a time, so each finds the order the last left); the copies show once made.
    fn duplicate(&mut self, dirs: Vec<String>) {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let (store, (done, duplicating)) = (Arc::clone(&self.store), std::sync::mpsc::channel());
        std::thread::spawn(move || {
            let _one = ONE_AT_A_TIME.lock();
            for dir in dirs {
                store.duplicate(dir);
            }
            let _ = done.send(());
        });
        self.duplicating.push(duplicating);
    }

    /// Duplicates made since the last frame shown (the gallery read again); looked at again soon
    /// while more are being made.
    fn duplicated(&mut self, ctx: &egui::Context) {
        let before = self.duplicating.len();
        self.duplicating.retain(|d| matches!(d.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
        if self.duplicating.len() < before {
            self.refresh();
        }
        if !self.duplicating.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// The thumbnails read since the last frame made textures; looked at again soon while more
    /// are coming.
    fn thumbnails_read(&mut self, ctx: &egui::Context) {
        let Some(reader) = &self.reader else { return };
        loop {
            match reader.read.try_recv() {
                Ok((dir, when, image)) => {
                    self.reading.remove(&(dir.clone(), when));
                    // enlarged, each of its pixels a square of its own colour, as the canvas shows them
                    let crisp = egui::TextureOptions {
                        magnification: egui::TextureFilter::Nearest,
                        ..egui::TextureOptions::LINEAR
                    };
                    let texture = image.map(|pixels| ctx.load_texture(format!("thumbnail {dir}"), pixels, crisp));
                    self.thumbs.insert(dir, (when, texture));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // its thread stopped: started again for those still wanted
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.reader = None;
                    self.reading.clear();
                    break;
                }
            }
        }
        if !self.reading.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    /// An artwork's thumbnail in `rect`, turned `turn` degrees: white under its picture, rounded
    /// 8 (but turned), with a soft shadow.
    fn thumbnail(&mut self, ui: &Ui, rect: Rect, a: &GalleryArtwork, turn: f32) {
        let texture = self.texture(a);
        let painter = ui.painter();
        if turn == 0.0 {
            let shadow = egui::epaint::Shadow { offset: [0, 5], blur: 8, spread: 0, color: argb(THUMB_SHADOW) };
            painter.add(shadow.as_shape(rect, 8));
            // a picture with its background hidden shows the gallery through it (white while it loads)
            painter.rect_filled(rect, 8, if texture.is_some() { argb(BACKGROUND) } else { Color32::WHITE });
            if let Some(t) = texture {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.add(egui::epaint::RectShape::filled(rect, 8, Color32::WHITE).with_texture(t, uv));
            }
            painter.rect_stroke(rect, 8, Stroke::new(1.0, argb(THUMB_RIM)), egui::StrokeKind::Inside);
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
    fn artwork_cell(&mut self, ui: &Ui, cell: Rect, a: &GalleryArtwork, scale: f32) -> Rect {
        let frame = Self::thumb_box(cell);
        let picture = Self::fitted(frame, a);
        self.pictures.insert(a.dir.clone(), picture);
        // shrunk or grown about its middle as it is pressed or lifted; spread open, it is drawn
        // where the fingers have it instead
        let picture = Rect::from_center_size(picture.center(), picture.size() * scale);
        if self.spreading.as_ref().is_none_or(|s| s.artwork.dir != a.dir) {
            self.thumbnail(ui, picture, a, 0.0);
        }
        let chosen = self.picked.contains(&a.dir);
        // the marks fade in as Select starts, and out as it ends
        let marks = ui.ctx().animate_bool_with_time_and_easing(
            Id::unique("select marks"),
            self.selecting,
            SELECT_FADE_S,
            |t| crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
        );
        if marks > 0.0 {
            // a circle at its top right: filled blue with a tick when picked, else a thin light ring
            let mark = pos2(picture.right() - 12.0 - 7.0, picture.top() + 12.0 + 7.0);
            if chosen {
                ui.painter().circle_filled(mark, 7.0, argb(PICKED).gamma_multiply(marks));
            } else {
                let ring = Stroke::new(1.0, argb(UNPICKED_RING).gamma_multiply(marks));
                ui.painter().circle_stroke(mark, 6.5, ring);
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
        // a row more for each kind of artwork a plugin makes
        let kinds_height = self.kinds.len() as f32 * (9.5 + 38.0);
        let height = panel::ARROW_HEIGHT + NEW_HEADER + 38.0 + 9.5 + list_height + kinds_height + 30.0;
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
            // the plugins' kinds of artwork under the presets, each the screen's size
            for (k, kind) in self.kinds.iter().enumerate() {
                let at = list.bottom() + 9.5 + k as f32 * (9.5 + 38.0);
                let line = Rect::from_min_size(pos2(row.left(), at), vec2(row.width(), 38.0));
                ui.painter().rect_filled(line, 19.0, argb(LIGHT_ROW_STRONG));
                preset_row(ui, line, &kind.label, &format!("{sw} \u{d7} {sh}px"), P3);
                if ui.interact(line, ui.scope_id().with(("plugin kind", k)), Sense::click()).clicked() {
                    asked = Some(Asked::PluginKind(kind.key.clone(), self.folder()));
                }
            }
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

/// The most memory (bytes) an animation's frames take in Preview: past it they are smaller.
const PLAYED_MOST: f64 = 256e6;

/// An artwork's frames as read for Preview, and how they play.
struct PlayedFrames {
    pictures: Vec<procraft_core::ThumbnailImage>,
    order: Vec<u32>,
    fps: u32,
    playback: procraft_core::AnimationPlayback,
}

/// An animated artwork in Preview: being read, then played.
struct Played {
    dir: String,
    read: Option<std::sync::mpsc::Receiver<Option<PlayedFrames>>>,
    frames: Vec<TextureHandle>,
    order: Vec<u32>,
    fps: u32,
    playback: procraft_core::AnimationPlayback,
    at: u32,
    step: i32,
    due: Option<Instant>,
}

impl Played {
    /// Starts reading the artwork in `dir`, if it plays, at no more than `side` pixels.
    fn start(dir: String, side: u32) -> Played {
        let (done, read) = std::sync::mpsc::channel();
        let (from, plays) = (dir.clone(), procraft_core::artwork_plays(dir.clone()));
        if plays {
            std::thread::spawn(move || _ = done.send(read_frames(from, side)));
        }
        Played {
            dir,
            read: plays.then_some(read),
            frames: Vec::new(),
            order: Vec::new(),
            fps: 1,
            playback: procraft_core::AnimationPlayback::Loop,
            at: 0,
            step: 1,
            due: None,
        }
    }

    /// Takes in what was read, and moves on when the time has come.
    fn go_on(&mut self, ctx: &egui::Context) {
        if let Some(read) = &self.read {
            match read.try_recv() {
                Ok(frames) => {
                    self.read = None;
                    if let Some(f) = frames {
                        self.frames = f
                            .pictures
                            .iter()
                            .enumerate()
                            .map(|(i, t)| {
                                let size = [t.width as usize, t.height as usize];
                                let image = egui::ColorImage::from_rgba_unmultiplied(size, &t.rgba);
                                ctx.load_texture(format!("played {i}"), image, egui::TextureOptions::LINEAR)
                            })
                            .collect();
                        (self.order, self.fps, self.playback) = (f.order, f.fps.max(1), f.playback);
                        self.due = Some(Instant::now());
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.read = None,
            }
        }
        let Some(due) = self.due else { return };
        let frame = Duration::from_secs_f64(1.0 / f64::from(self.fps));
        let now = Instant::now();
        if now < due {
            ctx.request_repaint_after(due - now);
            return;
        }
        let len = u32::try_from(self.order.len()).unwrap_or(0);
        match procraft_core::animation_next_step(self.playback, len, self.at, self.step) {
            Some(next) => {
                (self.at, self.step) = (next.at, next.step);
                self.due = Some(now + frame);
                ctx.request_repaint_after(frame);
            }
            // a single pass stays on its last frame
            None => self.due = None,
        }
    }

    /// The frame showing, once there are frames.
    fn frame(&self) -> Option<egui::TextureId> {
        let index = *self.order.get(self.at as usize)? as usize;
        self.frames.get(index).map(TextureHandle::id)
    }
}

/// The frames of the artwork in `dir`, at no more than `side` pixels and [`PLAYED_MOST`] together.
fn read_frames(dir: String, side: u32) -> Option<PlayedFrames> {
    let session = procraft_core::CanvasSession::open(dir).ok()?;
    // a model goes round
    if session.model_info().is_some() {
        if !session.draw_without_window() {
            return None;
        }
        let count = session.model_preview_frames();
        let fit = (PLAYED_MOST / 4.0 / f64::from(count.max(1))).sqrt() as u32;
        let pictures: Vec<_> = (0..count).filter_map(|i| session.model_preview_picture(i, side.min(fit))).collect();
        let order = (0..pictures.len() as u32).collect();
        let (fps, playback) = (session.model_preview_fps(), procraft_core::AnimationPlayback::Loop);
        return Some(PlayedFrames { pictures, order, fps, playback });
    }
    let settings = session.animation_settings().filter(|s| !s.pages)?;
    if !session.draw_without_window() {
        return None;
    }
    let count = session.animation_frame_count();
    let fit = (PLAYED_MOST / 4.0 / f64::from(count.max(1))).sqrt() as u32;
    let pictures: Vec<_> = (0..count).filter_map(|i| session.animation_frame(i, false, side.min(fit))).collect();
    Some(PlayedFrames { pictures, order: session.animation_sequence(), fps: settings.fps, playback: settings.playback })
}

/// A thread reading the gallery's thumbnails one after another: an artwork's folder, when it was
/// changed and its picture's file asked; the folder, the time and the picture (none if it can't
/// be read) sent back.
struct ThumbnailReader {
    asks: std::sync::mpsc::Sender<(String, i64, std::path::PathBuf)>,
    read: std::sync::mpsc::Receiver<(String, i64, Option<egui::ColorImage>)>,
}

impl ThumbnailReader {
    fn start() -> ThumbnailReader {
        let (asks, asked) = std::sync::mpsc::channel::<(String, i64, std::path::PathBuf)>();
        let (send, read) = std::sync::mpsc::channel();
        // it ends when the gallery lets go of it
        let _ = std::thread::Builder::new().name("thumbnails".into()).spawn(move || {
            for (dir, when, path) in asked {
                let image = crate::files::read_png(&path)
                    .ok()
                    .filter(|image| image.rgba.len() == image.width as usize * image.height as usize * 4)
                    .map(|image| {
                        let size = [image.width as usize, image.height as usize];
                        egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba)
                    });
                if send.send((dir, when, image)).is_err() {
                    return;
                }
            }
        });
        ThumbnailReader { asks, read }
    }
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2, vec2};

    use super::formats_beside;

    #[test]
    fn share_formats_go_where_there_is_room_and_point_at_the_pill() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0));
        let size = vec2(260.0, 515.0);
        // a pill on the left: the list on its right, its arrow at the pill's middle
        let pill = Rect::from_min_size(pos2(40.0, 300.0), vec2(94.0, 56.0));
        let (rect, arrow_y, right) = formats_beside(pill, screen, size);
        assert!(right);
        assert_eq!(rect.left(), pill.right() + 2.0);
        assert!((rect.top() + arrow_y - pill.center().y).abs() < 0.01);
        // a pill near the right edge: on its left, its right end 2 off the pill
        let pill = Rect::from_min_size(pos2(1300.0, 300.0), vec2(94.0, 56.0));
        let (rect, _, right) = formats_beside(pill, screen, size);
        assert!(!right);
        assert!((rect.right() - (pill.left() - 2.0)).abs() < 0.01);
        // a pill at the very top: kept 2 inside, the arrow no nearer its end than the corner allows
        let pill = Rect::from_min_size(pos2(40.0, 0.0), vec2(94.0, 20.0));
        let (rect, arrow_y, _) = formats_beside(pill, screen, size);
        assert_eq!(rect.top(), 2.0);
        assert!(arrow_y >= crate::panel::POPOVER_CORNER + crate::panel::ARROW_HEIGHT);
    }
}
