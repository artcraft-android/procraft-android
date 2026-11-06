//! The interface's look, as the Android app has it (ui/Theme.kt, ui/Type.kt, ui/Icons.kt): its
//! colours and sizes (points, which are its dp), the Inter type it bundles, and its icons and
//! shapes, drawn at the size they show.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, TextureHandle, TextureOptions, Ui, pos2, vec2};
use resvg::tiny_skia;

use crate::icons::Icon;

/// The light interface (Prefs) is on: panels and bars go light grey with dark text.
static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

/// A colour the interface shows as the first (0xAARRGGBB) while dark, the second while light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Themed(pub u32, pub u32);

/// A colour as written: the same either way (0xAARRGGBB), or one for each look.
pub trait Shade: Copy {
    /// The colour now, 0xAARRGGBB.
    fn value(self) -> u32;
}

impl Shade for u32 {
    fn value(self) -> u32 {
        self
    }
}

impl Shade for Themed {
    fn value(self) -> u32 {
        if light() { self.1 } else { self.0 }
    }
}

/// The same colour either way.
impl From<u32> for Themed {
    fn from(v: u32) -> Themed {
        Themed(v, v)
    }
}

/// A colour written 0xAARRGGBB, as the Android app writes them (as it is now, for one with a
/// light look).
pub fn argb(v: impl Shade) -> Color32 {
    let [a, r, g, b] = v.value().to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// `a` turned `t` (0..1) of the way to `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), ch(a.a(), b.a()))
}

/// The interface's colours (0xAARRGGBB, dark and light) and sizes (points), named as the Android
/// app's `Pc`.
pub mod pc {
    use super::Themed;

    pub const ACCENT: u32 = 0xFF0076FD;
    /// Text and a menu's words: white, nearly black (black for a menu's) on the light interface.
    pub const TEXT: Themed = Themed(0xFFFFFFFF, 0xFF222222);
    pub const MENU_INK: Themed = Themed(0xFFFFFFFF, 0xFF000000);
    pub const BAR: Themed = Themed(0xE61C1C1C, 0xCFE6E6E6);
    pub const BAR_TEXT: Themed = Themed(0xFFAAAAAA, 0xFF3B3B3B);
    pub const BAR_GLYPH: Themed = Themed(0xFFC7C7C7, 0xFF373737);
    pub const BAR_GLYPH_SELECTED: u32 = 0xFF0076FD;
    pub const ACTION_DISC: Themed = Themed(0xFF141414, 0xFFEDEDED);
    pub const ACTION_GLYPH: Themed = Themed(0xFF969696, 0xFF646464);
    pub const COLOR_RING: Themed = Themed(0xFF969696, 0xFF696969);
    pub const BAR_HEIGHT: f32 = 44.0;
    pub const BAR_MARGIN: f32 = 40.0;
    pub const BAR_BUTTON_WIDTH: f32 = 44.0;
    pub const BAR_BUTTON_HEIGHT: f32 = 38.0;
    pub const GALLERY_WIDTH: f32 = 58.0;
    pub const ACTION_DISC_SIZE: f32 = 32.0;
    pub const COLOR_DOT: f32 = 31.0;

    pub const SIDE_BAR: Themed = Themed(0xE7171717, 0xCFE5E5E5);
    pub const SLIDER_TRACK: Themed = Themed(0xC93E3E3E, 0xBFF0F0F0);
    pub const SLIDER_RIM: Themed = Themed(0xB96E6E6E, 0xFFE9E9E9);
    pub const SLIDER_RIM_WIDTH: f32 = 0.8;
    pub const SLIDER_THUMB: Themed = Themed(0xFFA6A6A6, 0xFFF8F8F8);
    pub const SIDE_GLYPH: Themed = Themed(0xFF969696, 0xFF646464);
    pub const SIDE_GLYPH_DISABLED: Themed = Themed(0x1AFFFFFF, 0x33000000);
    pub const SIDE_BAR_WIDTH: f32 = 51.0;
    pub const SIDE_BAR_HEIGHT: f32 = 519.0;
    pub const SIDE_BAR_CORNER: f32 = 20.0;
    /// Where the side bar's centre sits, as a fraction of the window's height.
    pub const SIDE_BAR_CENTRE: f32 = 0.5234;
    /// Left under the side bar in a window too short for its measured tracks, and how short they go.
    const SHORT_SIDE_BAR_FOOT: f32 = 17.0;
    const SHORTEST_TRACK: f32 = 44.0;
    pub const SLIDER_WIDTH: f32 = 40.0;
    pub const SLIDER_HEIGHT: f32 = 170.0;
    pub const SLIDER_CORNER: f32 = 13.0;
    pub const THUMB_WIDTH: f32 = 32.0;
    pub const THUMB_HEIGHT: f32 = 18.0;
    pub const THUMB_INSET: f32 = 4.0;
    pub const MODIFY_SIZE: f32 = 22.5;
    pub const MODIFY_STROKE: f32 = 2.4;
    pub const MODIFY_CORNER: f32 = 6.0;

    pub const GLASS_MENU_EDGE: Themed = Themed(0x24FFFFFF, 0x14000000);

    /// The gaps between the tool buttons and between the action buttons for a bar `width` wide:
    /// the room left after both margins picks one of five steps, or where its two ends would run
    /// into each other (a narrow window), the first tighter one that keeps them apart.
    pub fn bar_gaps(width: f32) -> (f32, f32) {
        let room = width - BAR_MARGIN * 2.0;
        let measured = if room < 435.0 {
            (2.0, 4.0)
        } else if room < 530.0 {
            (8.0, 14.0)
        } else if room < 625.0 {
            (26.0, 14.0)
        } else if room < 730.0 {
            (8.0, 14.0)
        } else {
            (26.0, 14.0)
        };
        let needs = |(tool, action): (f32, f32)| {
            BAR_MARGIN * 2.0 + GALLERY_WIDTH + (BAR_BUTTON_WIDTH + action) * 4.0 + BAR_BUTTON_WIDTH * 5.0 + tool * 4.0
        };
        [measured, (8.0, 14.0)].into_iter().find(|&gaps| needs(gaps) <= width).unwrap_or((2.0, 4.0))
    }

    /// The side bar in a window `height` tall: its top from the window's, and how long each of its
    /// two tracks is. At `SIDE_BAR_CENTRE` with its measured tracks wherever that clears the top
    /// bar; in a shorter window just under the top bar, its tracks giving up what it is short of,
    /// so that all of it can be reached.
    pub fn side_bar_fit(height: f32) -> (f32, f32) {
        let top = height * SIDE_BAR_CENTRE - SIDE_BAR_HEIGHT / 2.0;
        if top >= BAR_HEIGHT {
            return (top, SLIDER_HEIGHT);
        }
        let rest = SIDE_BAR_HEIGHT - SLIDER_HEIGHT * 2.0;
        (BAR_HEIGHT, ((height - BAR_HEIGHT - SHORT_SIDE_BAR_FOOT - rest) / 2.0).clamp(SHORTEST_TRACK, SLIDER_HEIGHT))
    }
}

/// How long a button takes to light up, and to go back (seconds).
pub const HIGHLIGHT_ON: f32 = 0.067;
pub const HIGHLIGHT_OFF: f32 = 0.15;

/// How lit `id` is (0..1): it lights up fast and goes back more slowly.
pub fn highlight(ctx: &egui::Context, id: egui::Id, on: bool) -> f32 {
    let time = if on { HIGHLIGHT_ON } else { HIGHLIGHT_OFF };
    ctx.animate_value_with_time(id, if on { 1.0 } else { 0.0 }, time)
}

/// Letter spacing that brings Inter to the measured text widths (em).
const TRACKING: f32 = -0.045;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    fn name(self) -> &'static str {
        match self {
            Weight::Regular => "Inter-Regular",
            Weight::Medium => "Inter-Medium",
            Weight::SemiBold => "Inter-SemiBold",
            Weight::Bold => "Inter-Bold",
        }
    }

    pub fn font(self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(self.name().into()))
    }
}

/// Inter's faces (bundled with the Android app, OFL).
pub static INTER_FACES: [(Weight, &[u8]); 4] = [
    (Weight::Regular, include_bytes!("../../android/app/src/main/assets/fonts/Inter-Regular.ttf")),
    (Weight::Medium, include_bytes!("../../android/app/src/main/assets/fonts/Inter-Medium.ttf")),
    (Weight::SemiBold, include_bytes!("../../android/app/src/main/assets/fonts/Inter-SemiBold.ttf")),
    (Weight::Bold, include_bytes!("../../android/app/src/main/assets/fonts/Inter-Bold.ttf")),
];

/// Inter (bundled with the Android app, OFL) for all text, egui's own fonts behind it for what it
/// lacks, and the system's behind those for other scripts (read on a thread of their own, put in
/// once read).
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(fonts(Vec::new()));
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let system = system_fonts();
        if !system.is_empty() {
            ctx.set_fonts(fonts(system));
            ctx.request_repaint();
        }
    });
}

/// The app's fonts, with `system`'s `(name, font)` after egui's own.
fn fonts(system: Vec<(String, egui::FontData)>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let mut fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, font) in system {
        fonts.font_data.insert(name.clone(), Arc::new(font));
        if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
            mono.push(name.clone());
        }
        fallback.push(name);
    }
    for &(weight, bytes) in &INTER_FACES {
        let name = weight.name().to_owned();
        fonts.font_data.insert(name.clone(), Arc::new(egui::FontData::from_static(bytes)));
        let mut family = vec![name];
        family.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(weight.name().into()), family);
    }
    fonts
        .families
        .insert(FontFamily::Proportional, [Weight::Regular.name().to_owned()].into_iter().chain(fallback).collect());
    fonts
}

/// A Mac's fonts for the scripts Inter and egui's own lack (as the other systems' below), found by
/// their families through Core Text (where the files are differs from one release to the next):
/// `(name, font)` each, a collection's own face of the family.
#[cfg(target_os = "macos")]
fn system_fonts() -> Vec<(String, egui::FontData)> {
    const FAMILIES: [&str; 5] =
        ["PingFang SC", "Hiragino Sans GB", "Apple SD Gothic Neo", "Arial Unicode MS", "Apple Symbols"];
    let mut read: Vec<(std::path::PathBuf, String)> = Vec::new();
    FAMILIES
        .iter()
        .filter_map(|family| {
            let (file, face) = core_text::font_file(family)?;
            // two families in one face are read once
            if read.contains(&(file.clone(), face.clone())) {
                return None;
            }
            let bytes = std::fs::read(&file).ok()?;
            // its face in a collection, by its PostScript name
            let index = swash::FontDataRef::new(&bytes)
                .and_then(|data| {
                    data.fonts().position(|font| {
                        let name = font.localized_strings().find_by_id(swash::StringId::PostScript, None);
                        name.is_some_and(|name| name.to_string() == face)
                    })
                })
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(0);
            read.push((file, face));
            let mut font = egui::FontData::from_owned(bytes);
            font.index = index;
            Some((format!("system {family}"), font))
        })
        .collect()
}

/// Core Text's calls for the fonts' files, unsafe as the system's are.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod core_text {
    use std::path::PathBuf;

    use objc2_core_foundation::{CFString, CFURL};
    use objc2_core_text::{CTFont, kCTFontURLAttribute};

    /// The file of the font family `family` and the PostScript name of its face, if the system
    /// has it (asked for one it lacks, Core Text gives another family).
    pub fn font_file(family: &str) -> Option<(PathBuf, String)> {
        // SAFETY: no matrix (the font as it is); the attribute asked of it is its file's URL
        unsafe {
            let font = CTFont::with_name(&CFString::from_str(family), 12.0, std::ptr::null());
            if font.family_name().to_string() != family {
                return None;
            }
            let url = font.attribute(kCTFontURLAttribute)?.downcast::<CFURL>().ok()?;
            Some((url.to_file_path()?, font.post_script_name().to_string()))
        }
    }
}

/// The system's fonts for the scripts Inter and egui's own lack (Chinese, Japanese, Korean,
/// Arabic, Hebrew, Indic, Thai, ...), those of them it has: names (a brush's from another app,
/// an artwork's typed in any language) shown as written. `(name, font)` each.
#[cfg(not(target_os = "macos"))]
fn system_fonts() -> Vec<(String, egui::FontData)> {
    #[cfg(windows)]
    let (dir, files): (std::path::PathBuf, &[&str]) = (
        std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts"),
        &[
            "msyh.ttc",
            "malgun.ttf",
            "segoeui.ttf",
            "Nirmala.ttc",
            "LeelawUI.ttf",
            "ebrima.ttf",
            "gadugi.ttf",
            "seguisym.ttf",
        ],
    );
    #[cfg(unix)]
    let (dir, files): (std::path::PathBuf, &[&str]) = (
        std::path::PathBuf::from("/usr/share/fonts"),
        &[
            "opentype/noto/NotoSansCJK-Regular.ttc",
            "noto-cjk/NotoSansCJK-Regular.ttc",
            "google-noto-cjk/NotoSansCJK-Regular.ttc",
            "truetype/droid/DroidSansFallbackFull.ttf",
            "truetype/noto/NotoSansArabic-Regular.ttf",
            "truetype/noto/NotoSansHebrew-Regular.ttf",
            "truetype/noto/NotoSansDevanagari-Regular.ttf",
            "truetype/noto/NotoSansThai-Regular.ttf",
            "truetype/dejavu/DejaVuSans.ttf",
        ],
    );
    #[cfg(not(any(windows, unix)))]
    let (dir, files): (std::path::PathBuf, &[&str]) = (std::path::PathBuf::new(), &[]);
    files
        .iter()
        .filter_map(|file| {
            let bytes = std::fs::read(dir.join(file)).ok()?;
            // a collection's first face (each of these has its regular weight first)
            Some((format!("system {file}"), egui::FontData::from_owned(bytes)))
        })
        .collect()
}

/// Text in runs of weights, `size` points, placed by `align` at `at`; returns where it went.
pub fn text_runs(
    painter: &Painter,
    at: Pos2,
    align: Align2,
    runs: &[(&str, Weight)],
    size: f32,
    colour: Color32,
) -> Rect {
    tracked_runs(painter, at, align, runs, size, colour, TRACKING)
}

/// The tracking (em) that brings the face to the measured widths of rows' labels near 15 points,
/// where [`TRACKING`] (set over 18-24 point labels) draws them narrower.
pub const ROW_TRACKING: f32 = -0.024;

/// Text of one weight, its letters `tracking` em apart; see [`text_runs`].
#[allow(clippy::too_many_arguments)]
pub fn text_tracked(
    painter: &Painter,
    at: Pos2,
    align: Align2,
    text: &str,
    size: f32,
    weight: Weight,
    colour: Color32,
    tracking: f32,
) -> Rect {
    tracked_runs(painter, at, align, &[(text, weight)], size, colour, tracking)
}

fn tracked_runs(
    painter: &Painter,
    at: Pos2,
    align: Align2,
    runs: &[(&str, Weight)],
    size: f32,
    colour: Color32,
    tracking: f32,
) -> Rect {
    let mut job = LayoutJob::default();
    for (text, weight) in runs {
        let format = TextFormat {
            font_id: weight.font(size),
            color: colour,
            extra_letter_spacing: tracking * size,
            ..Default::default()
        };
        job.append(text, 0.0, format);
    }
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(at, galley.size());
    painter.galley(rect.min, galley, colour);
    rect
}

/// Text of one weight, `size` points, wrapped `width` wide, laid out to be drawn.
pub fn wrapped(
    painter: &Painter,
    text: &str,
    size: f32,
    weight: Weight,
    colour: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let format = TextFormat {
        font_id: weight.font(size),
        color: colour,
        extra_letter_spacing: TRACKING * size,
        ..Default::default()
    };
    job.append(text, 0.0, format);
    job.wrap.max_width = width;
    painter.layout_job(job)
}

/// Text wrapped `width` wide with its lines centred (about x = 0 where it is drawn) and
/// `line_height` apart.
pub fn wrapped_centred(
    painter: &Painter,
    text: &str,
    size: f32,
    weight: Weight,
    colour: Color32,
    width: f32,
    line_height: f32,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let format = TextFormat {
        font_id: weight.font(size),
        color: colour,
        extra_letter_spacing: TRACKING * size,
        line_height: Some(line_height),
        ..Default::default()
    };
    job.append(text, 0.0, format);
    job.wrap.max_width = width;
    job.halign = egui::Align::Center;
    painter.layout_job(job)
}

/// How far across (points) the pointer has gone since the last frame while held on `response`, a
/// slider; `held` keeps which slider is held and where the pointer was.
pub fn dragged_by(held: &mut Option<(egui::Id, f32)>, response: &egui::Response) -> f32 {
    match response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on()) {
        Some(p) => {
            let moved = match *held {
                Some((id, x)) if id == response.id => p.x - x,
                _ => 0.0,
            };
            *held = Some((response.id, p.x));
            moved
        }
        None => {
            if held.is_some_and(|(id, _)| id == response.id) {
                *held = None;
            }
            0.0
        }
    }
}

/// How wide `text` of one weight is at `size` points, as [`text`] draws it.
pub fn text_width(painter: &Painter, text: &str, size: f32, weight: Weight) -> f32 {
    let mut job = LayoutJob::default();
    let format = TextFormat { font_id: weight.font(size), extra_letter_spacing: TRACKING * size, ..Default::default() };
    job.append(text, 0.0, format);
    painter.layout_job(job).size().x
}

/// Text of one weight; see [`text_runs`].
pub fn text(
    painter: &Painter,
    at: Pos2,
    align: Align2,
    text: &str,
    size: f32,
    weight: Weight,
    colour: Color32,
) -> Rect {
    text_runs(painter, at, align, &[(text, weight)], size, colour)
}

/// The whole of a texture.
const ALL: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1.0, 1.0) };

/// Icons and shapes drawn for the screen's density, kept until it changes.
#[derive(Default)]
pub struct Look {
    icons: HashMap<(&'static str, u32), TextureHandle>,
    /// Each drawn shape's texture and what it was drawn from.
    shapes: HashMap<&'static str, (u64, TextureHandle)>,
}

impl Look {
    /// `icon` shown `width` points across (its frame scaled to it), centred at `centre`, in `colour`.
    pub fn icon(&mut self, ui: &Ui, icon: &Icon, centre: Pos2, width: f32, colour: Color32) {
        let ppp = ui.ctx().pixels_per_point();
        let px = (width * ppp).round().max(1.0) as u32;
        let texture = self
            .icons
            .entry((icon.name, px))
            .or_insert_with(|| ui.ctx().load_texture(icon.name, raster_icon(icon, px), TextureOptions::LINEAR));
        let height = width * icon.height / icon.width;
        ui.painter().image(texture.id(), Rect::from_center_size(centre, vec2(width, height)), ALL, colour);
    }

    /// `icon` stretched to fill `rect` (drawn as wide as it, then squeezed or stretched to its
    /// height), in `colour`.
    pub fn icon_in(&mut self, ui: &Ui, icon: &Icon, rect: Rect, colour: Color32) {
        let ppp = ui.ctx().pixels_per_point();
        let px = (rect.width() * ppp).round().max(1.0) as u32;
        let texture = self
            .icons
            .entry((icon.name, px))
            .or_insert_with(|| ui.ctx().load_texture(icon.name, raster_icon(icon, px), TextureOptions::LINEAR));
        ui.painter().image(texture.id(), rect, ALL, colour);
    }

    /// A shape drawn by `draw` (into a pixmap of `rect` at the screen's density, scaled so its
    /// units are points from `rect`'s corner), kept as `slot` until `key` changes.
    pub fn shape(
        &mut self,
        ui: &Ui,
        slot: &'static str,
        key: impl Hash,
        rect: Rect,
        draw: impl FnOnce(&mut tiny_skia::Pixmap, tiny_skia::Transform),
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = ((rect.width() * ppp).ceil().max(1.0) as u32, (rect.height() * ppp).ceil().max(1.0) as u32);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        // drawn again when the interface turns light or dark
        (key, w, h, light()).hash(&mut hasher);
        let hash = hasher.finish();
        let fresh = self.shapes.get(slot).is_none_or(|(at, _)| *at != hash);
        if fresh && let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) {
            draw(&mut pixmap, tiny_skia::Transform::from_scale(ppp, ppp));
            let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data());
            match self.shapes.get_mut(slot) {
                Some((at, texture)) => {
                    texture.set(image, TextureOptions::LINEAR);
                    *at = hash;
                }
                None => {
                    let texture = ui.ctx().load_texture(slot, image, TextureOptions::LINEAR);
                    self.shapes.insert(slot, (hash, texture));
                }
            }
        }
        if let Some((_, texture)) = self.shapes.get(slot) {
            let shown = Rect::from_min_size(rect.min, vec2(w as f32, h as f32) / ppp);
            ui.painter().image(texture.id(), shown, ALL, Color32::WHITE);
        }
    }
}

/// `icon` drawn white `px` pixels across.
fn raster_icon(icon: &Icon, px: u32) -> egui::ColorImage {
    let mut svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}">"#, icon.width, icon.height);
    for part in icon.parts {
        if part.fill {
            let rule = if part.even_odd { "evenodd" } else { "nonzero" };
            svg.push_str(&format!(r##"<path d="{}" fill="#fff" fill-rule="{rule}"/>"##, part.d));
        }
        if part.stroke > 0.0 {
            svg.push_str(&format!(
                r##"<path d="{}" fill="none" stroke="#fff" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"/>"##,
                part.d, part.stroke
            ));
        }
    }
    svg.push_str("</svg>");
    let ph = ((px as f32) * icon.height / icon.width).round().max(1.0) as u32;
    let drawn = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok().and_then(|tree| {
        let mut pixmap = tiny_skia::Pixmap::new(px, ph)?;
        let scale = px as f32 / icon.width;
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        Some(pixmap)
    });
    match drawn {
        Some(p) => egui::ColorImage::from_rgba_premultiplied([px as usize, ph as usize], p.data()),
        None => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
    }
}

/// A rounded rectangle's outline: corners `[top left, top right, bottom right, bottom left]`.
pub fn round_rect(pb: &mut tiny_skia::PathBuilder, r: Rect, [tl, tr, br, bl]: [f32; 4]) {
    // the control points' share of a quarter circle's radius
    const K: f32 = 0.552_284_8;
    let (x0, y0, x1, y1) = (r.left(), r.top(), r.right(), r.bottom());
    pb.move_to(x0 + tl, y0);
    pb.line_to(x1 - tr, y0);
    pb.cubic_to(x1 - tr + tr * K, y0, x1, y0 + tr - tr * K, x1, y0 + tr);
    pb.line_to(x1, y1 - br);
    pb.cubic_to(x1, y1 - br + br * K, x1 - br + br * K, y1, x1 - br, y1);
    pb.line_to(x0 + bl, y1);
    pb.cubic_to(x0 + bl - bl * K, y1, x0, y1 - bl + bl * K, x0, y1 - bl);
    pb.line_to(x0, y0 + tl);
    pb.cubic_to(x0, y0 + tl - tl * K, x0 + tl - tl * K, y0, x0 + tl, y0);
    pb.close();
}

/// Fills `path` in `colour`, even-odd.
pub fn fill(pixmap: &mut tiny_skia::Pixmap, path: Option<tiny_skia::Path>, colour: Color32, at: tiny_skia::Transform) {
    let Some(path) = path else { return };
    let mut paint = tiny_skia::Paint::default();
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::EvenOdd, at, None);
}

/// A cubic Bézier easing from (0, 0) to (1, 1) through (`x1`, `y1`) and (`x2`, `y2`), as CSS and
/// Compose give them, at `t`.
/// A spring settling over its own time (`seconds`, as damped as `damping`, starting at
/// `velocity`), as the Android app's (ui/Motion.kt, Spring): how far along it is at `fraction`
/// (0..1) of that time, overshooting while it is under-damped.
pub fn spring(seconds: f32, damping: f32, velocity: f32, fraction: f32) -> f32 {
    if fraction >= 1.0 {
        return 1.0;
    }
    let z = f64::from(damping).max(0.01);
    let v = f64::from(velocity);
    // how far from its end it can still be at time `t` at frequency `w`
    let envelope = |w: f64, t: f64| {
        if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let c2 = (v - z * w) / wd;
            (1.0 + c2 * c2).sqrt() * (-z * w * t).exp()
        } else if z == 1.0 {
            (1.0 + (v - w).abs() * t) * (-w * t).exp()
        } else {
            let s = (z * z - 1.0).sqrt();
            let (r1, r2) = (-w * (z - s), -w * (z + s));
            let c1 = (v + r2) / (r1 - r2);
            c1.abs() * (r1 * t).exp() + (-1.0 - c1).abs() * (r2 * t).exp()
        }
    };
    // the slowest frequency that is within a thousandth of its end by the time given
    let seconds = f64::from(seconds);
    let (mut lo, mut hi) = (0.01, 2000.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if envelope(mid, seconds) > 1e-3 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let (w, t) = (hi, f64::from(fraction.max(0.0)) * seconds);
    let off = if z < 1.0 {
        let wd = w * (1.0 - z * z).sqrt();
        (-z * w * t).exp() * (-(wd * t).cos() + (v - z * w) / wd * (wd * t).sin())
    } else if z == 1.0 {
        (-1.0 + (v - w) * t) * (-w * t).exp()
    } else {
        let s = (z * z - 1.0).sqrt();
        let (r1, r2) = (-w * (z - s), -w * (z + s));
        let c1 = (v + r2) / (r1 - r2);
        c1 * (r1 * t).exp() + (-1.0 - c1) * (r2 * t).exp()
    };
    (1.0 + off) as f32
}

pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let curve = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3);
    let slope =
        |a: f32, b: f32, s: f32| 3.0 * a * (1.0 - s).powi(2) + 6.0 * (b - a) * s * (1.0 - s) + 3.0 * (1.0 - b) * s * s;
    // where along the curve x is t: Newton's method, then halving if that didn't settle
    let mut s = t;
    for _ in 0..8 {
        let (dx, d) = (curve(x1, x2, s) - t, slope(x1, x2, s));
        if dx.abs() < 1e-5 || d.abs() < 1e-6 {
            break;
        }
        s = (s - dx / d).clamp(0.0, 1.0);
    }
    if (curve(x1, x2, s) - t).abs() >= 1e-4 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            s = (lo + hi) / 2.0;
            if curve(x1, x2, s) < t {
                lo = s;
            } else {
                hi = s;
            }
        }
    }
    curve(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::{pc, spring};

    #[test]
    fn the_top_bars_ends_never_meet_and_a_wide_enough_window_keeps_its_measured_step() {
        assert_eq!(pc::bar_gaps(720.0), (8.0, 14.0));
        assert_eq!(pc::bar_gaps(1200.0), (26.0, 14.0));
        // the step for this width has gaps of 26 and 14, which would take 694
        assert_eq!(pc::bar_gaps(650.0), (8.0, 14.0));
        assert_eq!(pc::bar_gaps(610.0), (2.0, 4.0));
        for width in 558..2000 {
            let width = width as f32;
            let (tool, action) = pc::bar_gaps(width);
            let needs = pc::BAR_MARGIN * 2.0
                + pc::GALLERY_WIDTH
                + (pc::BAR_BUTTON_WIDTH + action) * 4.0
                + pc::BAR_BUTTON_WIDTH * 5.0
                + tool * 4.0;
            assert!(needs <= width, "{width}");
        }
    }

    #[test]
    fn the_side_bar_stays_as_measured_or_fits_under_the_top_bar() {
        // portrait top 333.5, landscape top 130
        assert!((pc::side_bar_fit(1133.0).0 - 333.5).abs() < 0.1);
        assert!((pc::side_bar_fit(744.0).0 - 130.0).abs() < 0.1);
        for height in 580..2000 {
            assert_eq!(pc::side_bar_fit(height as f32).1, pc::SLIDER_HEIGHT);
        }
        for height in 480..580 {
            let height = height as f32;
            let (top, track) = pc::side_bar_fit(height);
            assert_eq!(top, pc::BAR_HEIGHT);
            assert!(top + pc::SIDE_BAR_HEIGHT - (pc::SLIDER_HEIGHT - track) * 2.0 <= height, "{height}");
        }
    }

    #[test]
    fn a_spring_starts_still_overshoots_and_settles() {
        assert!(spring(0.3, 0.75, 0.0, 0.0).abs() < 1e-3);
        assert!((spring(0.3, 0.75, 0.0, 0.999) - 1.0).abs() < 0.01);
        assert_eq!(spring(0.3, 0.75, 0.0, 1.0), 1.0);
        // under-damped, it goes past its end on the way
        assert!((0..100).any(|i| spring(0.3, 0.75, 0.0, i as f32 / 100.0) > 1.0));
        // damped enough, it doesn't
        assert!((0..100).all(|i| spring(0.3, 1.2, 0.0, i as f32 / 100.0) <= 1.0));
    }
}
