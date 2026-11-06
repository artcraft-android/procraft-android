//! Text drawn in the core: a text layer's text laid out (wrapped, aligned and spaced) and its
//! letters drawn into the layer's pixels, with the fonts each app gives (its bundled face, the
//! system's, the user's own), so both apps draw text alike. Laid out as the Android app's
//! TextRenderer did with the platform's own layout: the box's top at the text's origin less its
//! baseline, each line as tall as the font's ascent and descent plus the leading, the letters
//! spaced by the tracking and kerning (in sizes), an outline as a stroke a twenty-fifth of the
//! size wide.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, CacheKeyFlags, Family, FontSystem, LayoutGlyph, Metrics, Shaping, Style,
    SwashCache, SwashContent, Weight, fontdb,
};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Angle, Format, Stroke, Transform, Vector};

use crate::text::{TextAlignment, TextInfo};

/// The bundled face, as text layers name it.
const INTER: &str = "inter";
/// An imported font's name in a text layer: this, then its file's path; a family of a collection
/// other than its first, then `#` and where in the file its faces start.
const FILE: &str = "file:";
/// An outline's width, as a fraction of the size.
const OUTLINE: f32 = 0.04;

/// The fonts text is drawn with, the glyphs drawn so far, the names standing for families (the
/// generic ones a platform maps to its own), and the files read in (path to the name of each of
/// its families and where in the file its faces start).
struct Fonts {
    system: FontSystem,
    glyphs: SwashCache,
    strokes: ScaleContext,
    aliases: HashMap<String, String>,
    files: HashMap<String, Vec<(String, u32)>>,
}

/// Where the user's imported fonts are kept, and the typefaces they offer (by name): apart from
/// the fonts, so listing them never waits on text being drawn.
static IMPORTED: Mutex<(Option<std::path::PathBuf>, Vec<TextTypeface>)> = Mutex::new((None, Vec::new()));

fn imported() -> MutexGuard<'static, (Option<std::path::PathBuf>, Vec<TextTypeface>)> {
    IMPORTED.lock().unwrap_or_else(|e| e.into_inner())
}

fn fonts() -> MutexGuard<'static, Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Mutex::new(Fonts {
                system: FontSystem::new_with_locale_and_db("en-US".into(), fontdb::Database::new()),
                glyphs: SwashCache::new(),
                strokes: ScaleContext::new(),
                aliases: HashMap::new(),
                files: HashMap::new(),
            })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The family names of faces `ids`.
fn families_of(db: &fontdb::Database, ids: &[fontdb::ID]) -> Vec<String> {
    let mut names: Vec<String> =
        ids.iter().filter_map(|id| db.face(*id)).filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
    names.dedup();
    names
}

/// Adds the fonts in `bytes` (a .ttf, .otf or a collection) to those text is drawn with; the
/// families they are.
#[uniffi::export]
pub fn add_text_font(bytes: Vec<u8>) -> Vec<String> {
    let mut f = fonts();
    let db = f.system.db_mut();
    let ids = db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
    families_of(db, &ids)
}

/// Adds the fonts in the file at `path`; the families they are (none if it can't be read).
#[uniffi::export]
pub fn add_text_font_file(path: String) -> Vec<String> {
    let mut f = fonts();
    add_file(&mut f, &path)
}

/// The family of the file at `path` whose faces start at `start`, as text layers name it.
fn file_family(path: &str, start: u32) -> String {
    if start == 0 { format!("{FILE}{path}") } else { format!("{FILE}{path}#{start}") }
}

fn add_file(f: &mut Fonts, path: &str) -> Vec<String> {
    let db = f.system.db_mut();
    let ids = db.load_font_source(fontdb::Source::File(path.into()));
    // the file's faces under families of their own (their names in text layers), so text set in
    // one draws in this file and not in another font of the same family
    let mut families: Vec<(String, u32)> = Vec::new();
    for id in ids {
        let Some(mut info) = db.face(id).cloned() else { continue };
        let name = info.families.first().map(|(n, _)| n.clone()).unwrap_or_default();
        let start = families.iter().find(|(n, _)| *n == name).map_or(info.index, |&(_, start)| start);
        if !families.iter().any(|(n, _)| *n == name) {
            families.push((name, start));
        }
        db.remove_face(id);
        info.families = vec![(file_family(path, start), fontdb::Language::English_UnitedStates)];
        db.push_face_info(info);
    }
    let names = families.iter().map(|(name, _)| name.clone()).collect();
    if !families.is_empty() {
        f.files.insert(path.to_owned(), families);
    }
    names
}

/// The file a text layer's font names (`file:` then a path): the file of that name in the
/// imported fonts' folder, if it is there. A path anywhere else (another device's, or wherever a
/// received artwork says) is never opened.
fn font_file(font: &str) -> Option<std::path::PathBuf> {
    let path = font.strip_prefix(FILE)?;
    let name = path.rsplit(['/', '\\']).next()?.split('#').next();
    let name = name.filter(|n| !n.is_empty() && *n != "." && *n != "..")?;
    let at = imported().0.clone()?.join(name);
    at.is_file().then_some(at)
}

/// What a font file ends in: TrueType, OpenType, and collections of them.
const FONT_ENDINGS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// Whether `ending` is a font file's.
fn font_ending(ending: &str) -> bool {
    FONT_ENDINGS.iter().any(|e| ending.eq_ignore_ascii_case(e))
}

/// Whether `path` is named as a font file.
fn is_font(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(font_ending)
}

/// The font file at `path` read in and offered, in name order, as a typeface named by the file, or
/// a collection's families as typefaces by their own names; its first, none if it isn't a font.
fn offer_file(path: &std::path::Path) -> Option<TextTypeface> {
    let at = path.to_string_lossy().into_owned();
    let families = {
        let mut f = fonts();
        if !f.files.contains_key(&at) {
            add_file(&mut f, &at);
        }
        f.files.get(&at).cloned().unwrap_or_default()
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let alone = families.len() == 1;
    let name = |family: String| if alone { stem.clone() } else { family };
    let offered: Vec<TextTypeface> = families
        .into_iter()
        .map(|(family, start)| TextTypeface { name: name(family), family: file_family(&at, start) })
        .collect();
    let list = &mut imported().1;
    for typeface in &offered {
        if !list.contains(typeface) {
            list.push(typeface.clone());
        }
    }
    list.sort_by_key(|t| t.name.to_lowercase());
    offered.into_iter().next()
}

/// Where the user's imported fonts are kept (a folder of the app's), the fonts already there read
/// in and offered (once a folder).
#[uniffi::export]
pub fn set_text_font_folder(dir: String) {
    let dir = std::path::PathBuf::from(dir);
    if imported().0.replace(dir.clone()).as_ref() == Some(&dir) {
        return;
    }
    let mut kept: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| is_font(p)).collect())
        .unwrap_or_default();
    kept.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    for path in kept {
        offer_file(&path);
    }
}

/// A font file the user chose (`bytes`, called `file_name`) kept in the fonts folder and offered:
/// the typeface text can take. None if it isn't a font (nothing is kept) or there is no folder.
#[uniffi::export]
pub fn import_text_font(bytes: Vec<u8>, file_name: String) -> Option<TextTypeface> {
    let dir = imported().0.clone()?;
    let safe: String = file_name.chars().filter(|c| c.is_alphanumeric() || " ._-".contains(*c)).collect();
    // its name and ending as the file's (TrueType unless it says otherwise)
    let (stem, ending) = match safe.rsplit_once('.') {
        Some((stem, ext)) if font_ending(ext) => (stem, ext.to_lowercase()),
        _ => (safe.as_str(), "ttf".to_owned()),
    };
    let stem = stem.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = if stem.is_empty() { "Font" } else { stem };
    std::fs::create_dir_all(&dir).ok()?;
    // a font kept already under that name: the same file is offered again; another takes a name
    // of its own ("Name 2"), nothing kept overwritten
    let mut path = dir.join(format!("{stem}.{ending}"));
    for n in 2.. {
        match std::fs::read(&path) {
            Ok(kept) if kept == bytes => return offer_file(&path),
            Ok(_) => path = dir.join(format!("{stem} {n}.{ending}")),
            Err(_) => break,
        }
    }
    std::fs::write(&path, bytes).ok()?;
    // a file that isn't a font is dropped (only this new one)
    let offered = offer_file(&path);
    if offered.is_none() {
        let _ = std::fs::remove_file(&path);
    }
    offered
}

/// The generic families text layers name (Android's), each drawn in the first of these the
/// system has: on Android its own, as its font list gives them.
const ANDROID_GENERIC: [(&str, &[&str]); 8] = [
    ("sans-serif", &["Roboto"]),
    ("sans-serif-condensed", &["Roboto Condensed", "Roboto"]),
    ("serif", &["Noto Serif"]),
    ("monospace", &["Droid Sans Mono"]),
    ("serif-monospace", &["Cutive Mono"]),
    ("casual", &["Coming Soon"]),
    ("cursive", &["Dancing Script"]),
    ("sans-serif-smallcaps", &["Carrois Gothic SC", "Roboto"]),
];

/// The same on a computer: Windows', macOS's and Linux's nearest.
const COMPUTER_GENERIC: [(&str, &[&str]); 8] = [
    ("sans-serif", &["Segoe UI", "Helvetica Neue", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Arial"]),
    ("sans-serif-condensed", &["Arial Narrow", "Helvetica Neue", "DejaVu Sans Condensed", "Liberation Sans Narrow"]),
    ("serif", &["Times New Roman", "Times", "DejaVu Serif", "Liberation Serif", "Noto Serif", "Georgia"]),
    ("monospace", &["Consolas", "Menlo", "DejaVu Sans Mono", "Liberation Mono", "Courier New"]),
    ("serif-monospace", &["Courier New", "Courier", "Liberation Mono", "FreeMono"]),
    ("casual", &["Comic Sans MS", "Chalkboard SE", "Comic Neue"]),
    ("cursive", &["Segoe Script", "Snell Roundhand", "Apple Chancery", "URW Chancery L"]),
    ("sans-serif-smallcaps", &["Segoe UI", "Helvetica Neue", "DejaVu Sans", "Liberation Sans", "Arial"]),
];

/// Adds the system's own fonts, the generic families text layers name pointed at those of them
/// it has.
#[uniffi::export]
pub fn add_system_text_fonts() {
    let mut fonts = fonts();
    let db = fonts.system.db_mut();
    db.load_system_fonts();
    // the font library looks nowhere on Android: its fonts are in these folders
    if cfg!(target_os = "android") {
        for dir in ["/system/fonts", "/product/fonts", "/system_ext/fonts"] {
            db.load_fonts_dir(dir);
        }
    }
    let have: std::collections::HashSet<String> =
        db.faces().filter_map(|f| f.families.first().map(|(name, _)| name.clone())).collect();
    let generic = if cfg!(target_os = "android") { ANDROID_GENERIC } else { COMPUTER_GENERIC };
    for (name, choices) in generic {
        if let Some(found) = choices.iter().find(|c| have.contains(**c)) {
            fonts.aliases.insert(name.to_owned(), (*found).to_owned());
        }
    }
}

/// Text layers naming `name` (a platform's generic family, such as "serif") are drawn in
/// `family`.
#[uniffi::export]
pub fn set_text_family_alias(name: String, family: String) {
    fonts().aliases.insert(name, family);
}

/// Every family text can be drawn in, sorted, each once.
#[uniffi::export]
pub fn text_families() -> Vec<String> {
    let f = fonts();
    let mut names: Vec<String> =
        f.system.db().faces().filter_map(|face| face.families.first().map(|(n, _)| n.clone())).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The family a text layer's font name stands for.
fn family_name(f: &mut Fonts, font: &str) -> String {
    if font == INTER {
        return "Inter".to_owned();
    }
    if font.starts_with(FILE) {
        // an imported font, as kept in the fonts folder; missing, the sans serif
        if let Some(at) = font_file(font).map(|p| p.to_string_lossy().into_owned()) {
            if !f.files.contains_key(&at) {
                add_file(f, &at);
            }
            // a family the file has, else its first
            let start = font.rsplit_once('#').and_then(|(_, start)| start.parse().ok()).unwrap_or(0);
            if let Some(families) = f.files.get(&at) {
                let start = if families.iter().any(|&(_, s)| s == start) { start } else { 0 };
                return file_family(&at, start);
            }
        }
        return f.aliases.get("sans-serif").cloned().unwrap_or_else(|| "sans-serif".to_owned());
    }
    f.aliases.get(font).cloned().unwrap_or_else(|| font.to_owned())
}

/// A typeface the text tool offers: the name shown, and the family text layers name.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextTypeface {
    pub name: String,
    pub family: String,
}

/// The typefaces every platform offers (each draws the generic ones in its own fonts), then the
/// bundled Inter, then the user's imported fonts by name.
#[uniffi::export]
pub fn text_typefaces() -> Vec<TextTypeface> {
    let imported = imported().1.clone();
    [
        ("Sans Serif", "sans-serif"),
        ("Sans Serif Condensed", "sans-serif-condensed"),
        ("Serif", "serif"),
        ("Monospace", "monospace"),
        ("Serif Monospace", "serif-monospace"),
        ("Casual", "casual"),
        ("Cursive", "cursive"),
        ("Small Caps", "sans-serif-smallcaps"),
        ("Inter", INTER),
    ]
    .into_iter()
    .map(|(name, family)| TextTypeface { name: name.to_owned(), family: family.to_owned() })
    .chain(imported)
    .collect()
}

/// The styles each typeface offers, by name, lightest first, each upright then slanting.
#[uniffi::export]
pub fn text_styles() -> Vec<String> {
    ["Thin", "Light", "Regular", "Medium", "Bold", "Black"]
        .into_iter()
        .flat_map(|w| [w.to_owned(), if w == "Regular" { "Italic".to_owned() } else { format!("{w} Italic") }])
        .collect()
}

/// A style's weight and whether it slants, by its name ("Thin" to "Black Italic").
fn weight_and_slant(style: &str) -> (u16, bool) {
    let italic = style.ends_with("Italic");
    let weight = match style.split_whitespace().next().unwrap_or("") {
        "Thin" => 100,
        "Light" => 300,
        "Medium" => 500,
        "Bold" => 700,
        "Black" => 900,
        _ => 400,
    };
    (weight, italic)
}

/// What is drawn: upper case if asked, one letter a line when vertical (a blank line between the
/// text's own lines).
fn shown(t: &TextInfo) -> String {
    let text = if t.all_caps { t.text.to_uppercase() } else { t.text.clone() };
    if !t.vertical {
        return text;
    }
    use unicode_segmentation::UnicodeSegmentation;
    // each of its lines (an empty last one kept, as Android's layout keeps it) a letter a line,
    // a letter with its marks (or an emoji as joined) as one
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).graphemes(true).collect::<Vec<_>>().join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Text laid out: its lines, the box's size, how tall a line is and the font's ascent and descent
/// in it, the font's underline (offset below the baseline and thickness), all pixels; and how
/// the lines align.
struct Laid {
    lines: Vec<Line>,
    width: f32,
    height: f32,
    line_height: f32,
    ascent: f32,
    descent: f32,
    underline: (f32, f32),
    align: TextAlignment,
}

/// A line as laid out: its baseline from the box's top, where its letters start and how wide they
/// run, which of the text's own lines it is part of (a long one wraps onto several), and its
/// glyphs (each with where its letters are in that line, in bytes).
struct Line {
    baseline: f32,
    start: f32,
    width: f32,
    paragraph: usize,
    glyphs: Vec<LayoutGlyph>,
}

/// Lays `t` out with fonts `f`.
fn lay_out(f: &mut Fonts, t: &TextInfo) -> Laid {
    let size = t.size.max(1.0);
    // no font read in yet (an app draws text before giving any): nothing to lay out, rather than
    // the layout's panic for want of a font
    if f.system.db().is_empty() {
        let (ascent, descent) = (size * 0.97, size * 0.24);
        return Laid {
            lines: Vec::new(),
            width: t.width.max(1.0),
            height: ascent + descent,
            line_height: ascent + descent,
            ascent,
            descent,
            underline: (size * 0.1, 1.0),
            align: t.align,
        };
    }
    let family = family_name(f, &t.font);
    let (weight, italic) = weight_and_slant(&t.style);
    let mut attrs = Attrs::new()
        .family(Family::Name(&family))
        .weight(Weight(weight))
        .style(if italic { Style::Italic } else { Style::Normal })
        .letter_spacing(t.tracking + t.kerning);
    // the small capitals family: lower case as small capitals where the font has them (a
    // computer's stand-ins for Android's small-capitals face do)
    if t.font == "sans-serif-smallcaps" {
        let mut features = cosmic_text::FontFeatures::new();
        features.enable(cosmic_text::FeatureTag::SMALL_CAPS);
        attrs = attrs.font_features(features);
    }
    // the font's own ascent and descent, as a line's height before the leading
    let (ascent, descent, underline) = font_metrics(f, &family, weight, italic, size);
    let line_height = (ascent + descent + t.leading * size).max(1.0);
    let align = match t.align {
        TextAlignment::Left => Align::Left,
        TextAlignment::Center => Align::Center,
        TextAlignment::Right => Align::Right,
        TextAlignment::Justify => Align::Justified,
    };
    let text = shown(t);
    let mut buffer = Buffer::new(&mut f.system, Metrics::new(size, line_height));
    // without a width of its own, it is as wide as its longest line
    buffer.set_size(None, None);
    buffer.set_text(&text, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(&mut f.system, false);
    let natural = buffer.layout_runs().map(|r| r.line_w).fold(0.0f32, f32::max).ceil().max(1.0);
    let width = if t.width > 0.0 { t.width } else { natural };
    buffer.set_size(Some(width), None);
    buffer.shape_until_scroll(&mut f.system, false);
    // the letter spacing half before each letter and half after, as Android's layout spaces them
    // (the layout here puts it all after)
    let half = (t.tracking + t.kerning) * size / 2.0;
    let lines: Vec<Line> = buffer
        .layout_runs()
        .enumerate()
        .map(|(i, run)| Line {
            baseline: ascent + line_height * i as f32,
            start: run.glyphs.first().map_or(0.0, |g| g.x + half),
            width: run.line_w,
            paragraph: run.line_i,
            glyphs: run
                .glyphs
                .iter()
                .map(|g| {
                    let mut g = g.clone();
                    g.x += half;
                    g
                })
                .collect(),
        })
        .collect();
    // as Android's layout: every line its ascent and descent, the leading between lines only (the
    // last line takes none, more or less)
    let height = (line_height * lines.len().max(1) as f32 - t.leading * size).max(1.0);
    Laid { lines, width, height, line_height, ascent, descent, underline, align: t.align }
}

/// The font `family` (`weight`, `italic`) at `size`: its ascent, descent and underline (offset
/// below the baseline, thickness), pixels; a sans serif's if there is no such font.
fn font_metrics(f: &mut Fonts, family: &str, weight: u16, italic: bool, size: f32) -> (f32, f32, (f32, f32)) {
    let fallback = (size * 0.97, size * 0.24, (size * 0.1, (size / 14.0).max(1.0)));
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family), fontdb::Family::SansSerif],
        weight: fontdb::Weight(weight),
        stretch: fontdb::Stretch::Normal,
        style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
    };
    let Some(id) = f.system.db().query(&query) else { return fallback };
    let Some(font) = f.system.get_font(id, Weight(weight)) else { return fallback };
    let m = font.as_swash().metrics(&[]);
    let per_unit = size / f32::from(m.units_per_em.max(1));
    let underline = (-m.underline_offset * per_unit, (m.stroke_size * per_unit).max(1.0));
    (m.ascent * per_unit, m.descent * per_unit, underline)
}

/// The text's box on the canvas: its left, top, width and height (pixels).
#[uniffi::export]
pub fn text_box(info: TextInfo) -> Vec<f32> {
    let laid = lay_out(&mut fonts(), &info);
    vec![info.x, info.y - info.baseline * info.size, laid.width, laid.height]
}

/// A premultiplied RGBA8 picture `w` × `h` pixels, painted into a coverage at a time.
struct Picture {
    rgba: Vec<u8>,
    w: i32,
    h: i32,
}

impl Picture {
    fn new(w: u32, h: u32) -> Picture {
        let (w, h) = (w.max(1), h.max(1));
        Picture { rgba: vec![0; w as usize * h as usize * 4], w: w as i32, h: h as i32 }
    }

    /// Pixel (x, y) covered `a` (0..1) by `colour` (sRGB 0..1), over what is there.
    fn over(&mut self, x: i32, y: i32, colour: [f32; 3], a: f32) {
        let a = a.clamp(0.0, 1.0);
        if a <= 0.0 || x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let at = (y as usize * self.w as usize + x as usize) * 4;
        let Some(px) = self.rgba.get_mut(at..at + 4) else { return };
        for (v, c) in px.iter_mut().zip(colour.iter().chain(std::iter::once(&1.0))) {
            *v = (c * a * 255.0 + f32::from(*v) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Glyph image `image` with its origin at (x, y): each pixel it covers, and how much.
fn each_pixel(image: &swash::scale::image::Image, x: i32, y: i32, mut put: impl FnMut(i32, i32, f32)) {
    let (gx, gy) = (x + image.placement.left, y - image.placement.top);
    let (iw, ih) = (image.placement.width as i32, image.placement.height as i32);
    for j in 0..ih {
        for i in 0..iw {
            let k = (j * iw + i) as usize;
            let coverage = match image.content {
                SwashContent::Mask => image.data.get(k).map_or(0.0, |v| f32::from(*v) / 255.0),
                SwashContent::Color => image.data.get(k * 4 + 3).map_or(0.0, |v| f32::from(*v) / 255.0),
                SwashContent::SubpixelMask => image.data.get(k * 4).map_or(0.0, |v| f32::from(*v) / 255.0),
            };
            put(gx + i, gy + j, coverage);
        }
    }
}

/// Glyph `glyph` on the line whose baseline is at `baseline`, the box's left at `left` (pixels of
/// the picture drawn into): filled, or a stroke `stroke` wide round its outline; and where its
/// origin falls.
fn glyph_image(
    f: &mut Fonts,
    glyph: &LayoutGlyph,
    left: f32,
    baseline: f32,
    stroke: Option<f32>,
) -> Option<(swash::scale::image::Image, i32, i32)> {
    let physical = glyph.physical((left, baseline), 1.0);
    let image = match stroke {
        Some(width) => stroked(f, physical.cache_key, width),
        None => f.glyphs.get_image_uncached(&mut f.system, physical.cache_key),
    }?;
    Some((image, physical.x, physical.y))
}

/// Where `info`'s letters may reach on the canvas: its laid-out box and each letter where it
/// lands (tight kerning or tracking can take letters out of the box), with a letter's height
/// around them (slanted and overhanging letters, the underline, an outline).
pub(crate) fn text_reach(info: &TextInfo) -> procraft_engine::geom::IRect {
    let laid = lay_out(&mut fonts(), info);
    let (left, top) = (info.x, info.y - info.baseline * info.size);
    let margin = info.size.abs() + 4.0;
    let n = |v: f32| if v.is_finite() { v.clamp(-1e9, 1e9) as i32 } else { 0 };
    let around = |x0: f32, y0: f32, x1: f32, y1: f32| {
        procraft_engine::geom::IRect::new(
            n((x0 - margin).floor()),
            n((y0 - margin).floor()),
            n((x1 + margin).ceil()),
            n((y1 + margin).ceil()),
        )
    };
    let mut reach = around(left, top, left + laid.width, top + laid.height);
    for line in &laid.lines {
        let baseline = top + line.baseline;
        for glyph in &line.glyphs {
            let at = glyph.physical((left, baseline), 1.0);
            let (x, y) = (at.x as f32, at.y as f32);
            reach = reach.union(around(x, y - laid.ascent, x + glyph.w.max(0.0), y + laid.descent));
        }
    }
    reach
}

/// The text drawn over transparency within `region` of the canvas: that region's pixels only,
/// premultiplied RGBA8.
pub(crate) fn draw_text_within(info: &TextInfo, region: procraft_engine::geom::IRect) -> Vec<u8> {
    let mut out = Picture::new(region.width(), region.height());
    let mut guard = fonts();
    let f = &mut *guard;
    let laid = lay_out(f, info);
    let (left, top) = (info.x - region.x0 as f32, info.y - info.baseline * info.size - region.y0 as f32);
    letters(f, &laid, info, &mut out, (left, top), |_, _| true);
    out.rgba
}

/// The text drawn over transparency on a picture `width` × `height`, as the canvas is:
/// premultiplied RGBA8.
#[uniffi::export]
pub fn draw_text(info: TextInfo, width: u32, height: u32) -> Vec<u8> {
    let mut out = Picture::new(width, height);
    let mut guard = fonts();
    let f = &mut *guard;
    let laid = lay_out(f, &info);
    let (left, top) = (info.x, info.y - info.baseline * info.size);
    letters(f, &laid, &info, &mut out, (left, top), |_, _| true);
    out.rgba
}

/// The letters (and underline) of `t`, laid out as `laid`, drawn into `out` with the box's top
/// left at `at`, where `inside` lets them.
fn letters(
    f: &mut Fonts,
    laid: &Laid,
    t: &TextInfo,
    out: &mut Picture,
    at: (f32, f32),
    inside: impl Fn(i32, i32) -> bool,
) {
    let colour = [t.r, t.g, t.b].map(|c| c.clamp(0.0, 1.0));
    let opacity = t.opacity.clamp(0.0, 1.0);
    let stroke = t.outline.then(|| (t.size * OUTLINE).max(1.0));
    for line in &laid.lines {
        let y = at.1 + line.baseline;
        for glyph in &line.glyphs {
            let Some((image, x0, y0)) = glyph_image(f, glyph, at.0, y, stroke) else { continue };
            each_pixel(&image, x0, y0, |x, y, c| {
                if inside(x, y) {
                    out.over(x, y, colour, c * opacity);
                }
            });
        }
        // the underline under the whole line
        if t.underline && line.width > 0.0 {
            let (offset, thickness) = laid.underline;
            let (x0, x1) = ((at.0 + line.start).floor() as i32, (at.0 + line.start + line.width).ceil() as i32);
            let (y0, y1) =
                ((y + offset).round() as i32, (y + offset + thickness).round().max((y + offset).round() + 1.0) as i32);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    if inside(xx, yy) {
                        out.over(xx, yy, colour, opacity);
                    }
                }
            }
        }
    }
}

/// Character `index` of `text`: which of its lines it is on, and how far into that line (bytes).
/// A place in `text` counted in UTF-16 units (as Android's and Java's strings count, an emoji two)
/// as the characters the core's text calls take (`text_caret`, `draw_chosen_text`); a place
/// inside a pair counts as after it.
#[uniffi::export]
pub fn text_index_from_utf16(text: String, utf16: u32) -> u32 {
    let mut units = 0u32;
    let mut chars = 0u32;
    for c in text.chars() {
        if units >= utf16 {
            break;
        }
        units += c.len_utf16() as u32;
        chars += 1;
    }
    chars
}

/// A character place in `text` (as the core counts) in UTF-16 units, as Android's strings count.
#[uniffi::export]
pub fn text_index_to_utf16(text: String, index: u32) -> u32 {
    text.chars().take(index as usize).map(|c| c.len_utf16() as u32).sum()
}

/// Character `index` of the text as typed, as a character of the text shown: in capitals one
/// letter can show as several ("ß" as "SS").
fn shown_index(t: &TextInfo, index: usize) -> usize {
    if !t.all_caps {
        return index;
    }
    t.text.chars().take(index).map(|c| c.to_uppercase().count()).sum()
}

fn place_of(text: &str, index: usize) -> (usize, usize) {
    let byte = text.char_indices().nth(index).map_or(text.len(), |(b, _)| b);
    let before = text.get(..byte).unwrap_or("");
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (before.matches('\n').count(), byte - line_start)
}

/// Where the caret before byte `offset` of the text's line `paragraph` stands: which line as laid
/// out, and its x from the box's left.
fn caret_in(laid: &Laid, paragraph: usize, offset: usize) -> Option<(usize, f32)> {
    let mut last = None;
    for (i, line) in laid.lines.iter().enumerate().filter(|(_, l)| l.paragraph == paragraph) {
        if let Some(g) = line.glyphs.iter().find(|g| g.start <= offset && offset < g.end) {
            return Some((i, if g.level.is_rtl() { g.x + g.w } else { g.x }));
        }
        last = Some((i, line));
    }
    // past its last letter: after it, on its last line (an empty one by how it aligns)
    let (i, line) = last?;
    let x = match line.glyphs.last() {
        Some(g) if g.level.is_rtl() => g.x,
        Some(g) => g.x + g.w,
        None => match laid.align {
            TextAlignment::Center => laid.width / 2.0,
            TextAlignment::Right => laid.width,
            TextAlignment::Left | TextAlignment::Justify => 0.0,
        },
    };
    Some((i, x))
}

/// Where the caret stands before character `index` of the text: its x and its line's top and
/// bottom on the canvas (pixels). None for vertical text, whose letters are spread apart.
#[uniffi::export]
pub fn text_caret(info: TextInfo, index: u32) -> Vec<f32> {
    if info.vertical {
        return Vec::new();
    }
    let laid = lay_out(&mut fonts(), &info);
    let (paragraph, offset) = place_of(&shown(&info), shown_index(&info, index as usize));
    let Some((line, x)) = caret_in(&laid, paragraph, offset) else { return Vec::new() };
    let top = info.y - info.baseline * info.size + laid.line_height * line as f32;
    vec![info.x + x, top, top + laid.ascent + laid.descent]
}

/// How wide the ring round chosen letters is, as a fraction of the text's size.
const CHOSEN_RING: f32 = 0.08;

/// A picture of part of the canvas: its top left corner and size (pixels) and its premultiplied
/// RGBA8.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextPicture {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The letters from character `start` to `end` of the text, chosen: ringed in `ring` (ARGB) with
/// the letters over it in their own colour, cut to their lines' height; none if no letters are.
/// Vertical text shows none.
#[uniffi::export]
pub fn draw_chosen_text(info: TextInfo, start: u32, end: u32, ring: u32) -> Option<TextPicture> {
    let (start, end) = (start.min(end) as usize, start.max(end) as usize);
    if info.vertical || start == end {
        return None;
    }
    let mut guard = fonts();
    let f = &mut *guard;
    let laid = lay_out(f, &info);
    let text = shown(&info);
    let (from, to) = (place_of(&text, shown_index(&info, start)), place_of(&text, shown_index(&info, end)));
    let chosen = |paragraph: usize, g: &LayoutGlyph| {
        let after_start = paragraph > from.0 || (paragraph == from.0 && g.end > from.1);
        let before_end = paragraph < to.0 || (paragraph == to.0 && g.start < to.1);
        after_start && before_end
    };
    // each line's chosen stretch, from its line's top to its bottom (box pixels)
    let rects: Vec<[f32; 4]> = laid
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let picked = line.glyphs.iter().filter(|g| chosen(line.paragraph, g));
            let (x0, x1) = picked.fold((f32::MAX, f32::MIN), |(a, b), g| (a.min(g.x), b.max(g.x + g.w)));
            let top = laid.line_height * i as f32;
            (x0 < x1).then_some([x0, top, x1, top + laid.ascent + laid.descent])
        })
        .collect();
    let bounds = rects.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, r| {
        [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]
    });
    if rects.is_empty() {
        return None;
    }
    let (left, top) = (info.x, info.y - info.baseline * info.size);
    let (x, y) = ((left + bounds[0]).floor() as i32, (top + bounds[1]).floor() as i32);
    let (w, h) = (((left + bounds[2]).ceil() as i32 - x).max(1), ((top + bounds[3]).ceil() as i32 - y).max(1));
    let mut out = Picture::new(w as u32, h as u32);
    // the picture's pixels, back in box pixels: inside a chosen stretch
    let (ox, oy) = (left - x as f32, top - y as f32);
    let inside = |px: i32, py: i32| {
        let (bx, by) = (px as f32 + 0.5 - ox, py as f32 + 0.5 - oy);
        rects.iter().any(|r| bx >= r[0] && bx < r[2] && by >= r[1] && by < r[3])
    };
    let ring_colour = [(ring >> 16) & 255, (ring >> 8) & 255, ring & 255].map(|c| c as f32 / 255.0);
    let ring_alpha = ((ring >> 24) & 255) as f32 / 255.0;
    let width = (info.size * CHOSEN_RING).max(1.0);
    for line in &laid.lines {
        for glyph in line.glyphs.iter().filter(|g| chosen(line.paragraph, g)) {
            let Some((image, x0, y0)) = glyph_image(f, glyph, ox, oy + line.baseline, Some(width)) else { continue };
            each_pixel(&image, x0, y0, |px, py, c| {
                if inside(px, py) {
                    out.over(px, py, ring_colour, c * ring_alpha);
                }
            });
        }
    }
    // the letters again over their ring
    letters(f, &laid, &info, &mut out, (ox, oy), inside);
    Some(TextPicture { x, y, width: w as u32, height: h as u32, rgba: out.rgba })
}

/// Glyph `key` as a stroke `width` wide round its outline: the same outline its letter is filled
/// from (hinted alike, a variable font at the letter's weight, a slant made for a face with no
/// italic of its own).
fn stroked(f: &mut Fonts, key: CacheKey, width: f32) -> Option<swash::scale::image::Image> {
    let font = f.system.get_font(key.font_id, key.font_weight)?;
    let wght = swash::Tag::from_be_bytes(*b"wght");
    let mut builder = f
        .strokes
        .builder(font.as_swash())
        .size(f32::from_bits(key.font_size_bits))
        .hint(!key.flags.contains(CacheKeyFlags::DISABLE_HINTING));
    if let Some(axis) = font.as_swash().variations().find_by_tag(wght) {
        let weight = f32::from(key.font_weight.0).clamp(axis.min_value(), axis.max_value());
        builder = builder.normalized_coords(font.as_swash().variations().normalized_coords([(wght, weight)]));
    }
    let mut scaler = builder.build();
    Render::new(&[Source::Outline])
        .format(Format::Alpha)
        .offset(Vector::new(key.x_bin.as_float(), key.y_bin.as_float()))
        .transform(
            key.flags
                .contains(CacheKeyFlags::FAKE_ITALIC)
                .then(|| Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0))),
        )
        .style(Stroke::new(width))
        .render(&mut scaler, key.glyph_id)
}

#[cfg(test)]
mod tests {
    use super::{fonts, lay_out, shown};
    use crate::text::{TextAlignment, TextInfo};

    /// A font collection of the faces in `fonts`.
    fn collection(fonts: &[Vec<u8>]) -> Vec<u8> {
        let word = |at: usize, font: &[u8]| u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]]);
        let mut out = b"ttcf\0\x01\0\0".to_vec();
        out.extend((fonts.len() as u32).to_be_bytes());
        let mut at = 12 + 4 * fonts.len();
        for font in fonts {
            out.extend((at as u32).to_be_bytes());
            at += font.len().next_multiple_of(4);
        }
        let mut place = out.len();
        for font in fonts {
            // its tables are found from the start of the file
            let mut font = font.clone();
            for table in 0..usize::from(u16::from_be_bytes([font[4], font[5]])) {
                let entry = 12 + 16 * table + 8;
                let moved = word(entry, &font) + place as u32;
                font[entry..entry + 4].copy_from_slice(&moved.to_be_bytes());
            }
            place += font.len().next_multiple_of(4);
            font.resize(font.len().next_multiple_of(4), 0);
            out.extend(font);
        }
        out
    }

    #[test]
    fn a_collections_families_are_each_offered_and_text_set_in_one_draws_in_it() {
        let dir = std::env::temp_dir().join(format!("procraft-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::set_text_font_folder(dir.to_string_lossy().into_owned());
        let inter = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec();
        // the same font under a family of another name, wherever the file writes it
        let (from, to): (Vec<u8>, Vec<u8>) = (
            "Inter".encode_utf16().flat_map(u16::to_be_bytes).collect(),
            "Oxter".encode_utf16().flat_map(u16::to_be_bytes).collect(),
        );
        let mut other = inter.clone();
        for at in 0..other.len() - from.len() {
            if other[at..at + from.len()] == from[..] {
                other[at..at + to.len()].copy_from_slice(&to);
            }
        }
        let first = super::import_text_font(collection(&[inter, other]), "Two.ttc".to_owned()).unwrap();
        assert_eq!(first.name, "Inter");
        let offered = super::text_typefaces();
        let second = offered.iter().find(|t| t.name == "Oxter").expect("the second family");
        assert!(second.family.ends_with("Two.ttc#1"), "{second:?}");
        assert!(first.family.ends_with("Two.ttc"), "{first:?}");
        // each draws in its own family, a place the file hasn't in its first, and it reads back after
        let family = |font: &str| super::family_name(&mut fonts(), font);
        assert_eq!(family(&first.family), first.family);
        assert_eq!(family(&second.family), second.family);
        assert_eq!(family(&format!("{}#7", first.family)), first.family);
        // not a font: nothing kept
        assert!(super::import_text_font(b"not a font".to_vec(), "Bad.ttc".to_owned()).is_none());
        assert!(!dir.join("Bad.ttc").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn inter() {
        crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    }

    fn text(words: &str, width: f32) -> TextInfo {
        TextInfo {
            text: words.to_owned(),
            font: "inter".to_owned(),
            style: "Regular".to_owned(),
            size: 40.0,
            kerning: 0.0,
            tracking: 0.0,
            leading: 0.0,
            baseline: 0.0,
            opacity: 1.0,
            align: TextAlignment::Left,
            underline: false,
            outline: false,
            vertical: false,
            all_caps: false,
            r: 0.0,
            g: 0.0,
            b: 0.0,
            x: 0.0,
            y: 0.0,
            width,
        }
    }

    /// The text of each line as laid out.
    fn lines(t: &TextInfo) -> Vec<String> {
        let laid = lay_out(&mut fonts(), t);
        let words = shown(t);
        let paragraphs: Vec<&str> = words.split('\n').collect();
        laid.lines
            .iter()
            .map(|l| {
                let p = paragraphs[l.paragraph];
                let (a, b) = (l.glyphs.first().map_or(0, |g| g.start), l.glyphs.last().map_or(0, |g| g.end));
                p[a..b].to_owned()
            })
            .collect()
    }

    #[test]
    fn a_word_wider_than_the_box_breaks_by_letters_and_the_box_grows_down() {
        inter();
        let one = crate::text_box(text("Text", 0.0));
        let te = crate::text_box(text("Te", 0.0))[2];
        // words wrap whole where they fit
        assert_eq!(lines(&text("Text Text", one[2] + 5.0)), ["Text", "Text"]);
        // narrower than the word, it breaks between letters, the box as tall as the lines
        let narrow = text("Text", te + 1.0);
        assert_eq!(lines(&narrow), ["Te", "xt"]);
        let b = crate::text_box(narrow.clone());
        assert_eq!(b[2], te + 1.0);
        assert!((b[3] - 2.0 * one[3]).abs() < 1.0, "{} for a line {}", b[3], one[3]);
        // narrower than a letter, a letter a line
        assert_eq!(lines(&text("Text", 5.0)), ["T", "e", "x", "t"]);
    }
}
