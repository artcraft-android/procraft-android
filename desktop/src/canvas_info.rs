//! Canvas information, as the Android app's (ui/CanvasInfoUi.kt), over the whole window: the
//! pages (about the artwork, its dimensions, layers, colour profile, video and statistics) on a
//! card at the left (a rail of icons on an upright window), the artwork's name to edit as the
//! title, Cancel and Done. About keeps the author's photo and signature in the artwork's folder.

use std::path::Path;

use egui::{
    Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, TextureHandle, Ui, UiBuilder, pos2, vec2,
};
use procraft_core::{
    ArtworkInfo, CanvasSession, cmyk_profiles, color_profiles, convert_color, is_cmyk_profile, read_signature,
    write_photo, write_signature,
};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};

const SCREEN: Themed = Themed(0xFF1F1F1F, 0xFFF3F3F3);
const RAIL: Themed = Themed(0xFF272727, 0xFFEBEBEB);
const CARD: Themed = Themed(0xFF252525, 0xFFEBEBEB);
const INK: Themed = Themed(0xFFC1C1C1, 0xFF3B3B3B);
const RAIL_IDLE: Themed = Themed(0xFFAAAAAA, 0xFF3D3D3D);
const PHOTO: Themed = Themed(0xFF303030, 0xFFB3B3B3);
const PHOTO_GLYPH: Themed = Themed(0xFF4E4E4E, 0xFFF5F5F5);
const PLACEHOLDER: Themed = Themed(0xFF605F63, 0xFFB8B8BC);
const SIGN_LINE: Themed = Themed(0xFF494949, 0xFF9A9A9A);
const DONE: u32 = 0xFF0190FF;
const CARD_EDGE: Themed = Themed(0xFF373737, 0xFFDADADA);
/// "Sign here" and its clear button: the photo's glyph while dark, darker greys while light; the
/// header's words.
const SIGN_INK: Themed = Themed(0xFF4E4E4E, 0xFF666666);
const SIGN_CLEAR: Themed = Themed(0xFF4E4E4E, 0xFF393939);
const CAPSULE_INK: Themed = Themed(0xFFA5A5A5, 0xFF585858);
const PAGE_INK: Themed = Themed(0xFF797979, 0xFF3E3E3E);
const CARD_CORNER: f32 = 25.0;
/// Where a wide window's page starts and ends, beside the pages' card.
const WIDE_START: f32 = 358.0;
const WIDE_END: f32 = 28.0;

const PAGES: [(&str, &Icon, f32); 6] = [
    ("About this artwork", &icons::INFO_ABOUT, 39.0),
    ("Dimensions", &icons::INFO_DIMENSIONS, 51.0),
    ("Layers", &icons::INFO_LAYERS, 35.0),
    ("Color profile", &icons::INFO_PROFILE, 33.0),
    ("Video settings", &icons::INFO_VIDEO, 55.0),
    ("Statistics", &icons::INFO_STATISTICS, 35.0),
];

/// How Canvas information ended.
pub enum Ended {
    Cancel,
    /// Done; the colour profile to change the canvas to, if another was picked.
    Done(Option<String>),
}

pub struct CanvasInfo {
    page: usize,
    name: String,
    author: String,
    /// The artwork's folder in the gallery, where About keeps its photo and signature.
    dir: Option<String>,
    info: ArtworkInfo,
    /// The author's photo as shown, and a new one picked (kept on Done).
    photo: Option<TextureHandle>,
    photo_read: bool,
    new_photo: Option<image::RgbaImage>,
    /// The profile picked to change to.
    profile: String,
    /// The signature's strokes, x, y of each point across its box (0..1).
    signature: Vec<Vec<f32>>,
    /// The time-lapse recorded so far: its frames and what they take on disk (bytes), and its
    /// settings' names (quality, resolution, codec).
    lapse: (u32, u64),
    video: Vec<String>,
    /// Done with another profile picked asks first.
    asking: crate::alert::Alert,
    /// What couldn't be kept on Done, for the window to tell.
    failure: Option<&'static str>,
}

impl CanvasInfo {
    pub fn new(
        session: &CanvasSession,
        dir: Option<String>,
        lapse: (u32, u64),
        video: procraft_core::TimeLapseSettings,
    ) -> CanvasInfo {
        let info = session.artwork_info();
        CanvasInfo {
            page: 0,
            name: session.name(),
            author: info.author.clone(),
            signature: dir.clone().map(read_signature).unwrap_or_default(),
            dir,
            info,
            photo: None,
            photo_read: false,
            new_photo: None,
            profile: session.profile(),
            lapse,
            video: procraft_core::time_lapse_setting_names(video),
            asking: crate::alert::Alert::default(),
            failure: None,
        }
    }

    /// What couldn't be kept on Done (the photo or the signature), once.
    pub fn take_failure(&mut self) -> Option<&'static str> {
        self.failure.take()
    }

    /// Keeps what was changed: the name and author, and in the artwork's folder the photo and
    /// signature (each in the old one's place only once written whole).
    fn keep(&mut self, session: &CanvasSession) {
        if !self.name.trim().is_empty() && self.name.trim() != session.name() {
            session.set_name(self.name.clone());
        }
        session.set_author(self.author.clone());
        let Some(dir) = &self.dir else { return };
        if let Some(photo) = &self.new_photo {
            let rgb = image::DynamicImage::ImageRgba8(photo.clone()).to_rgb8();
            let mut jpeg = Vec::new();
            let written = rgb
                .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())
                .and_then(|()| write_photo(dir.clone(), jpeg).map_err(|e| e.to_string()));
            if let Err(e) = written {
                log::warn!("keeping the author's photo: {e}");
                self.failure = Some("The photo couldn't be kept.");
            }
        }
        if let Err(e) = write_signature(dir.clone(), self.signature.clone()) {
            log::warn!("keeping the signature: {e}");
            self.failure = self.failure.or(Some("The signature couldn't be kept."));
        }
    }

    /// Shows it over the whole window; returns how it ended, once it has.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<Ended> {
        if !self.photo_read {
            self.photo_read = true;
            let file = self.dir.as_ref().map(|d| Path::new(d).join("about").join("photo.jpg"));
            if let Some(picture) = file.and_then(|f| image::open(f).ok()) {
                self.photo = Some(photo_texture(ctx, &picture.to_rgba8()));
            }
        }
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let mut ended = None;
        let escape = crate::ui::escape(ctx) && ctx.memory(|m| m.focused().is_none());
        egui::Area::new(Id::unique("canvas info")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0, argb(SCREEN));
            let ink = argb(INK);
            if wide {
                // the pages by icon and name on a card headed "Canvas Info"
                let card = Rect::from_min_max(
                    screen.min + vec2(10.0, 10.0),
                    pos2(screen.left() + 330.0, screen.bottom() - 10.0),
                );
                painter.rect_filled(card, 20.0, argb(RAIL));
                painter.rect_stroke(card, 20.0, Stroke::new(1.0, argb(CARD_EDGE)), egui::StrokeKind::Inside);
                text(&painter, card.min + vec2(24.0, 19.0), Align2::LEFT_TOP, "Canvas Info", 24.0, Weight::Bold, ink);
                for (i, (label, icon, size)) in PAGES.into_iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(card.left() + 8.0, card.top() + 65.0 + 56.0 * i as f32),
                        vec2(304.0, 53.0),
                    );
                    let chosen = i == self.page;
                    if chosen {
                        painter.rect_filled(row, 26.5, argb(pc::ACCENT));
                    }
                    let tint = if chosen { Color32::WHITE } else { argb(RAIL_IDLE) };
                    look.icon(ui, icon, pos2(row.left() + 30.0, row.center().y), size, tint);
                    let label_ink = if chosen { Color32::WHITE } else { argb(PAGE_INK) };
                    text(
                        &painter,
                        pos2(row.left() + 60.0, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        17.0,
                        Weight::Regular,
                        label_ink,
                    );
                    if ui.interact(row, ui.scope_id().with(("page", i)), Sense::click()).clicked() {
                        self.page = i;
                    }
                }
            } else {
                // the rail of pages
                let rail = Rect::from_min_max(screen.min, pos2(screen.left() + 81.0, screen.bottom()));
                painter.rect_filled(rail, 0, argb(RAIL));
                for (i, (_, icon, size)) in PAGES.into_iter().enumerate() {
                    let tab = Rect::from_min_size(
                        pos2(rail.left() + 8.0, rail.top() + 46.0 + 56.0 * i as f32),
                        vec2(65.0, 53.0),
                    );
                    let chosen = i == self.page;
                    if chosen {
                        painter.rect_filled(tab, 26.5, argb(pc::ACCENT));
                    }
                    look.icon(ui, icon, tab.center(), size, if chosen { Color32::WHITE } else { argb(RAIL_IDLE) });
                    if ui.interact(tab, ui.scope_id().with(("page", i)), Sense::click()).clicked() {
                        self.page = i;
                    }
                }
            }
            // the name (centred over the page when wide)
            let field = if wide {
                Rect::from_min_max(
                    pos2(screen.left() + WIDE_START, screen.top() + 30.0),
                    pos2(screen.right() - WIDE_END, screen.top() + 54.0),
                )
            } else {
                Rect::from_min_size(pos2(screen.left() + 101.0, screen.top() + 30.0), vec2(400.0, 24.0))
            };
            egui::TextEdit::singleline(&mut self.name)
                .font(Weight::Bold.font(18.0))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .align(if wide { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER })
                .desired_width(field.width())
                .char_limit(256)
                .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
            // Cancel, Import before it on the colour profile page, and Done
            let done = Rect::from_min_size(pos2(screen.right() - 20.0 - 44.0, screen.top() + 20.0), vec2(44.0, 45.0));
            let words: &[&str] = if self.page == 3 { &["Import", "Cancel"] } else { &["Cancel"] };
            match crate::custom_canvas::capsule_words(ui, done.left() - 13.0, screen.top() + 21.0, words, CAPSULE_INK) {
                Some("Cancel") => ended = Some(Ended::Cancel),
                // a profile of the canvas's kind is picked to change to
                Some(_) => {
                    if let Some(name) =
                        crate::files::import_profile().filter(|n| is_cmyk_profile(n.clone()) == session.is_cmyk())
                    {
                        self.profile = name;
                    }
                }
                None => {}
            }
            painter.rect_filled(done, 22.0, argb(DONE));
            look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
            if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
                self.keep(session);
                let changed = (self.profile != session.profile()).then(|| self.profile.clone());
                // another profile is asked about first, as measured (a time-lapse recorded or not)
                if changed.is_some() {
                    self.asking.ask(crate::alert::Asking {
                        title: "Change color profile?".into(),
                        message: "Its colours keep their values and look as the new profile shows them, in the time-lapse recording too.".into(),
                        action: "Change".into(),
                        icon: None,
                        destructive: false,
                        extra: None,
                        first: Some("Don't change".into()),
                        alone: false,
                    });
                } else {
                    ended = Some(Ended::Done(changed));
                }
            }
            let (start, end) = if wide { (WIDE_START, WIDE_END) } else { (105.0, 24.0) };
            let page = Rect::from_min_max(
                pos2(screen.left() + start, screen.top() + 100.0),
                pos2(screen.right() - end, screen.bottom()),
            );
            ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                ScrollArea::vertical().id_salt(("info page", self.page)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    self.page(ui, look, session, page.width());
                    ui.add_space(40.0);
                });
            });
        });
        match self.asking.show(ctx, look) {
            Some(crate::alert::Answer::Action) => ended = Some(Ended::Done(Some(self.profile.clone()))),
            // kept in the profile it has (the rest kept)
            Some(_) => ended = Some(Ended::Done(None)),
            None => {}
        }
        if escape && !self.asking.is_open() {
            ended = Some(Ended::Cancel);
        }
        ended
    }

    fn page(&mut self, ui: &mut Ui, look: &mut Look, session: &CanvasSession, width: f32) {
        match self.page {
            0 => {
                self.about_card(ui, look, width);
                ui.add_space(21.0);
                let artwork_file = self.dir.as_ref().map(|d| Path::new(d).join("artwork.json"));
                let changed = artwork_file.and_then(|f| f.metadata().ok()).and_then(|m| m.modified().ok());
                let changed_ms =
                    changed.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
                let created = Some(self.info.created_ms as i64).filter(|&ms| ms > 0).or(changed_ms);
                let modified = changed_ms.or(created);
                let shown = |ms: Option<i64>| ms.map_or_else(|| "\u{2014}".to_owned(), date_text);
                rows(ui, width, &[("Date created", shown(created)), ("Date modified", shown(modified))], 50.0);
            }
            1 => {
                let c = session.info();
                let dpi = c.dpi.max(1.0);
                let mm = |px: u32| format!("{}mm", (px as f32 / dpi * 25.4).round() as i64);
                rows(
                    ui,
                    width,
                    &[("Pixel width", format!("{}px", c.width)), ("Pixel height", format!("{}px", c.height))],
                    50.0,
                );
                ui.add_space(21.0);
                rows(ui, width, &[("Physical width", mm(c.width)), ("Physical height", mm(c.height))], 50.0);
                ui.add_space(21.0);
                let shown = if dpi.fract() == 0.0 { format!("{}", dpi as i64) } else { format!("{dpi:.1}") };
                rows(ui, width, &[("DPI", shown)], 50.0);
            }
            2 => {
                let l = session.layer_stats();
                rows(ui, width, &[("Maximum layers", l.maximum.to_string())], 50.0);
                ui.add_space(21.0);
                let available = l.maximum.saturating_sub(l.used);
                let sets = session.texture_set_stats();
                if sets.is_empty() {
                    rows(
                        ui,
                        width,
                        &[("Layers used", l.used.to_string()), ("Layers available", available.to_string())],
                        50.0,
                    );
                } else {
                    // a model's layers are held by its texture sets, each up to the maximum
                    let each: Vec<_> =
                        sets.iter().map(|s| (s.name.as_str(), format!("{} of {}", s.used, s.maximum))).collect();
                    rows(ui, width, &each, 50.0);
                }
                ui.add_space(21.0);
                let kinds = [
                    ("Assisted layers", l.assisted.to_string()),
                    ("Clipping layers", l.clipping.to_string()),
                    ("Layer masks", l.masks.to_string()),
                    ("Layer groups", l.groups.to_string()),
                ];
                // a model has no drawing guide to assist with
                let shown = usize::from(session.model_info().is_some());
                rows(ui, width, kinds.get(shown..).unwrap_or(&kinds), 50.0);
            }
            3 => {
                let current = session.profile();
                rows(ui, width, &[("Current color profile", current.clone())], 44.0);
                ui.add_space(32.0);
                let (heading, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                text(
                    ui.painter(),
                    heading.left_center() + vec2(16.0, 0.0),
                    Align2::LEFT_CENTER,
                    "Available color profiles",
                    17.0,
                    Weight::Regular,
                    argb(INK),
                );
                ui.add_space(11.0);
                // the profiles of the canvas's kind; the one picked to change to fills its row blue
                let names = if session.is_cmyk() { cmyk_profiles() } else { color_profiles() };
                let height = 47.0 * names.len() as f32 + 2.0 * names.len().saturating_sub(1) as f32;
                let (card, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                ui.painter().rect_filled(card, CARD_CORNER, argb(CARD));
                for (i, name) in names.iter().enumerate() {
                    let row = Rect::from_min_size(pos2(card.left(), card.top() + 49.0 * i as f32), vec2(width, 47.0));
                    if i > 0 {
                        ui.painter().rect_filled(
                            Rect::from_min_size(row.left_top() - vec2(0.0, 2.0), vec2(width, 2.0)),
                            0,
                            argb(SCREEN),
                        );
                    }
                    let on = *name == self.profile && *name != current;
                    if on {
                        let corners = egui::CornerRadius {
                            nw: if i == 0 { 25 } else { 0 },
                            ne: if i == 0 { 25 } else { 0 },
                            sw: if i + 1 == names.len() { 25 } else { 0 },
                            se: if i + 1 == names.len() { 25 } else { 0 },
                        };
                        ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
                    }
                    let ink = if on { Color32::WHITE } else { argb(INK) };
                    text(
                        ui.painter(),
                        row.left_center() + vec2(13.0, 0.0),
                        Align2::LEFT_CENTER,
                        name,
                        14.0,
                        Weight::Regular,
                        ink,
                    );
                    if ui.interact(row, ui.scope_id().with(("profile", i)), Sense::click()).clicked() {
                        self.profile = name.clone();
                    }
                }
            }
            4 => {
                // the time-lapse recorded so far, as its video would be
                let (frames, bytes) = self.lapse;
                let seconds = frames / procraft_core::TIME_LAPSE_FPS;
                let video = [
                    ("Video length", format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)),
                    ("Video quality", self.video.first().cloned().unwrap_or_default()),
                    ("Video resolution", self.video.get(1).cloned().unwrap_or_default()),
                    ("Video file size", megabytes(bytes)),
                    ("Video codec", self.video.get(2).cloned().unwrap_or_default()),
                ];
                rows(ui, width, &video, 50.0);
            }
            _ => {
                let minutes = self.info.tracked_ms / 60_000;
                let size = self.dir.as_ref().map_or(0, |d| folder_size(Path::new(d)));
                let stats = [
                    ("Total strokes made", self.info.strokes.to_string()),
                    ("Tracked time", format!("{}h {}m", minutes / 60, minutes % 60)),
                    ("Total file size", megabytes(size)),
                ];
                rows(ui, width, &stats, 50.0);
            }
        }
    }

    /// The author: a photo (a tap picks another), "Made by" and the name, and a signature drawn
    /// over a dashed line.
    fn about_card(&mut self, ui: &mut Ui, look: &mut Look, width: f32) {
        let (card, _) = ui.allocate_exact_size(vec2(width, 300.0), Sense::hover());
        let painter = ui.painter().clone();
        painter.rect_filled(card, CARD_CORNER, argb(CARD));
        let photo = Rect::from_center_size(pos2(card.center().x, card.top() + 30.0 + 48.5), vec2(97.0, 97.0));
        painter.circle_filled(photo.center(), 48.5, argb(PHOTO));
        match &self.photo {
            Some(t) => {
                painter.add(
                    egui::epaint::RectShape::filled(photo, 48.5, Color32::WHITE)
                        .with_texture(t.id(), crop_uv(t.size())),
                );
            }
            None => look.icon(ui, &icons::PERSON, photo.center(), 48.0, argb(PHOTO_GLYPH)),
        }
        if ui.interact(photo, ui.scope_id().with("photo"), Sense::click()).clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_title("Photo")
                .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                .pick_file()
            && let Ok(picture) = image::open(path)
        {
            let rgba = picture.to_rgba8();
            self.photo = Some(photo_texture(ui.ctx(), &rgba));
            self.new_photo = Some(rgba);
        }
        // "Made by" and the name, centred together
        let field_width =
            if self.author.is_empty() { 60.0 } else { (self.author.chars().count() as f32 * 12.0 + 16.0).min(260.0) };
        let made_by = ui.painter().layout_no_wrap("Made by ".into(), Weight::Regular.font(21.0), argb(INK));
        let left = card.center().x - (made_by.size().x + field_width) / 2.0;
        let line_top = card.top() + 146.0;
        painter.galley(pos2(left, line_top), made_by.clone(), argb(INK));
        let field = Rect::from_min_size(pos2(left + made_by.size().x, line_top), vec2(field_width, made_by.size().y));
        if self.author.is_empty() {
            text(&painter, field.left_top(), Align2::LEFT_TOP, "Name", 21.0, Weight::Regular, argb(PLACEHOLDER));
        }
        egui::TextEdit::singleline(&mut self.author)
            .font(Weight::Regular.font(21.0))
            .text_color(argb(INK))
            .frame(egui::Frame::NONE)
            .margin(vec2(0.0, 0.0))
            .desired_width(field.width())
            .char_limit(256)
            .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
        // the signature, drawn over the dashed line
        let sign = Rect::from_min_size(pos2(card.center().x - 200.5, card.top() + 180.0), vec2(401.0, 121.0));
        let line_y = sign.top() + 82.0;
        let from = sign.left() + (401.0 - 305.0) / 2.0;
        let mut x = from;
        while x < from + 305.0 {
            painter.line_segment(
                [pos2(x, line_y), pos2((x + 3.0).min(from + 305.0), line_y)],
                Stroke::new(1.5, argb(SIGN_LINE)),
            );
            x += 7.0;
        }
        let response = ui.interact(sign, ui.scope_id().with("signature"), Sense::click_and_drag());
        // a drag draws a stroke from where it was pressed; a tap, a dot
        let across = |p: Pos2| [(p.x - sign.left()) / sign.width(), (p.y - sign.top()) / sign.height()];
        if let Some(p) = response.interact_pointer_pos() {
            if response.drag_started() {
                let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
                self.signature.push(across(from).into_iter().chain(across(p)).collect());
            } else if response.dragged()
                && let Some(stroke) = self.signature.last_mut()
            {
                stroke.extend(across(p));
            } else if response.clicked() {
                self.signature.push(across(p).to_vec());
            }
        }
        for stroke in &self.signature {
            let points: Vec<Pos2> = stroke
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[px, py]| pos2(sign.left() + px * sign.width(), sign.top() + py * sign.height()))
                .collect();
            if let [only] = points.as_slice() {
                painter.circle_filled(*only, 1.0, argb(INK));
            } else {
                painter.add(egui::Shape::line(points, Stroke::new(2.0, argb(INK))));
            }
        }
        text(
            &painter,
            sign.min + vec2(50.0, 90.0),
            Align2::LEFT_TOP,
            "Sign here",
            11.0,
            Weight::Regular,
            argb(SIGN_INK),
        );
        // always there, at the line's end
        let clear = Rect::from_min_size(sign.min + vec2(330.0, 58.0), vec2(22.0, 22.0));
        painter.circle_filled(clear.center(), 11.0, argb(SIGN_CLEAR));
        look.icon(ui, &icons::CROSS, clear.center(), 14.0, argb(CARD));
        if ui.interact(clear, ui.scope_id().with("clear signature"), Sense::click()).clicked() {
            self.signature.clear();
        }
    }
}

/// Rows of a name and its value in a rounded card, a line between.
fn rows(ui: &mut Ui, width: f32, rows: &[(&str, String)], tall: f32) {
    let height = tall * rows.len() as f32 + rows.len().saturating_sub(1) as f32;
    let (card, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(card, CARD_CORNER, argb(CARD));
    let ink = argb(INK);
    for (i, (label, value)) in rows.iter().enumerate() {
        let row = Rect::from_min_size(pos2(card.left(), card.top() + (tall + 1.0) * i as f32), vec2(width, tall));
        if i > 0 {
            ui.painter().rect_filled(
                Rect::from_min_size(row.left_top() - vec2(0.0, 1.0), vec2(width, 1.0)),
                0,
                argb(SCREEN),
            );
        }
        text(ui.painter(), row.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
        text(
            ui.painter(),
            row.right_center() - vec2(16.0, 0.0),
            Align2::RIGHT_CENTER,
            value,
            17.0,
            Weight::Regular,
            ink,
        );
    }
}

fn photo_texture(ctx: &egui::Context, rgba: &image::RgbaImage) -> TextureHandle {
    let size = [rgba.width() as usize, rgba.height() as usize];
    ctx.load_texture(
        "author photo",
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}

/// The middle square of a picture `size` across, as texture coordinates: what a round photo shows.
fn crop_uv([w, h]: [usize; 2]) -> Rect {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    if w > h {
        let k = (w - h) / 2.0 / w;
        Rect::from_min_max(pos2(k, 0.0), pos2(1.0 - k, 1.0))
    } else {
        let k = (h - w) / 2.0 / h;
        Rect::from_min_max(pos2(0.0, k), pos2(1.0, 1.0 - k))
    }
}

/// A moment as the page shows it: "October 30, 2026 at 3:15 PM", in local time.
fn date_text(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |t| t.format("%B %-d, %Y at %-I:%M %p").to_string())
}

fn megabytes(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_000_000.0)
}

/// What the files in `dir` (and its folders) take, bytes.
fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => folder_size(&e.path()),
                _ => e.metadata().map_or(0, |m| m.len()),
            })
            .sum()
    })
}

/// The colour the canvas's current colour becomes in profile `to` (from `from`), 0xRRGGBB.
pub fn colour_in(rgb: u32, from: &str, to: &str) -> u32 {
    convert_color(rgb, from.to_owned(), to.to_owned())
}
