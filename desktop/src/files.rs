//! Artworks on disk: new, open (ProCraft packages, PSDs and PNGs), save as .procraft, export,
//! and asking before unsaved changes are lost.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_core::{
    CanvasPreset, CanvasSession, KeptGestures, ModelFile, PagesOutcome, PsdOutcome, ThumbnailImage, export_file_name,
    import_pages, import_psd, preset_height, preset_width,
};
use rfd::{FileDialog, MessageDialog, MessageLevel};

use crate::ui::Interface;

/// What an artwork is shared as (the Actions panel's Share tab): written to a file the user picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// The layered package, as .procraft or with the other app's extension.
    Procraft,
    Package,
    Psd,
    Pdf,
    Png,
    Jpeg,
    Tiff,
    /// Each layer a page, or a PNG file in a folder.
    LayersPdf,
    LayerPngs,
    /// The current and picked layers, each a PNG file in a folder.
    ChosenLayerPngs,
    /// A model: as an OBJ zipped with its material file and textures, or as a USDZ; as shown, as
    /// a PNG, a JPEG or a TIFF; its texture sets, a PNG each in a folder.
    ModelObj,
    ModelUsdz,
    ModelRender,
    ModelJpeg,
    ModelTiff,
    ModelTextures,
    ModelArtwork,
    /// A plugin's artwork, its shapes kept.
    Svg,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Procraft | Format::ModelArtwork => "ProCraft",
            Format::Package => "Procreate",
            Format::Psd => "PSD",
            Format::Pdf | Format::LayersPdf => "PDF",
            Format::Png | Format::LayerPngs | Format::ChosenLayerPngs | Format::ModelRender | Format::ModelTextures => {
                "PNG"
            }
            Format::Tiff | Format::ModelTiff => "TIFF",
            Format::Jpeg | Format::ModelJpeg => "JPEG",
            Format::ModelObj => "OBJ",
            Format::ModelUsdz => "USDZ",
            Format::Svg => "SVG",
        }
    }

    /// A layered package: its file's name has underscores for spaces.
    fn package(self) -> bool {
        matches!(self, Format::Procraft | Format::Package | Format::ModelArtwork)
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Procraft | Format::ModelArtwork => "procraft",
            Format::Package => "procreate",
            Format::Psd => "psd",
            Format::Pdf | Format::LayersPdf => "pdf",
            Format::Png | Format::LayerPngs | Format::ChosenLayerPngs | Format::ModelRender | Format::ModelTextures => {
                "png"
            }
            Format::Tiff | Format::ModelTiff => "tiff",
            Format::Jpeg | Format::ModelJpeg => "jpg",
            Format::ModelObj => "zip",
            Format::Svg => "svg",
            Format::ModelUsdz => "usdz",
        }
    }
}

/// Where the app keeps what it keeps between runs (palettes and the like): the platform's place
/// for an app's data.
pub fn app_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("ProCraft")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/ProCraft")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("procraft")
    }
}

/// Shows a passing message with an OK button.
pub fn say(text: &str) {
    MessageDialog::new().set_level(MessageLevel::Info).set_title("ProCraft").set_description(text).show();
}

/// Whether the file at `path` holds brushes, by its name (a zip by what it holds): ours (a brush,
/// a set or a library) or another app's.
pub fn is_brush_file(path: &Path) -> bool {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ["brush", "brushset", "brushlibrary"].contains(&extension.as_str())
        || procraft_core::OTHER_BRUSH_EXTENSIONS.contains(&extension.as_str())
        // a brush or a set sent on as a plain zip (told by its names, not read whole)
        || (extension == "zip"
            && procraft_core::zip_file_kind(path.to_string_lossy().into_owned()) == procraft_core::ZipKind::Brushes)
}

/// Where brush files given while no canvas is open wait for one.
pub fn pending_brushes_dir() -> PathBuf {
    app_dir().join("pending-brushes")
}

/// Brush files given while no canvas is open, kept (under their own names, after a number that
/// keeps their order) until one opens; says so. False if none could be kept.
pub fn keep_pending_brushes(paths: &[PathBuf]) -> bool {
    let dir = pending_brushes_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut kept = false;
    for (i, path) in paths.iter().enumerate() {
        let Some(name) = path.file_name() else { continue };
        let to = dir.join(format!("{stamp}-{i}-{}", name.to_string_lossy()));
        kept |= std::fs::copy(path, to).is_ok();
    }
    if kept {
        say("Brushes will be added to the library when a canvas opens.");
    }
    kept
}

/// The brush files kept while no canvas was open, oldest first, each with its own name; taken
/// out of the folder.
pub fn take_pending_brushes() -> Vec<(Vec<u8>, String)> {
    let Ok(read) = std::fs::read_dir(pending_brushes_dir()) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok();
            let _ = std::fs::remove_file(&path);
            let kept = path.file_name()?.to_string_lossy().into_owned();
            // the name it came with, after the stamp and the number
            let name = kept.splitn(3, '-').nth(2).unwrap_or(&kept).to_owned();
            Some((bytes?, name))
        })
        .collect()
}

/// Shows a message with an OK button; it reaches the log and the terminal as well, for a system
/// with no way of showing it.
pub fn tell(title: &str, text: &str) {
    log::error!("{title}: {text}");
    eprintln!("{title}: {text}");
    if cfg!(target_os = "linux") && !has_program("zenity") {
        // what shows the app's dialogs on Linux isn't there: the first that is of the others
        let shown = other_dialogs(title, text).iter().any(|(program, args)| {
            std::process::Command::new(program).args(args).status().is_ok_and(|done| done.success())
        });
        if shown {
            return;
        }
    }
    MessageDialog::new().set_level(MessageLevel::Error).set_title(title).set_description(text).show();
}

/// Whether `program` can be run here.
fn has_program(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// The programs that can show a message where the app's own dialogs can't, in the order to try
/// them, each with its arguments.
fn other_dialogs(title: &str, text: &str) -> [(&'static str, Vec<String>); 3] {
    let (title, text) = (title.to_owned(), text.to_owned());
    [
        ("kdialog", vec!["--title".to_owned(), title.clone(), "--error".to_owned(), text.clone()]),
        ("xmessage", vec!["-center".to_owned(), format!("{title}\n\n{text}")]),
        ("notify-send", vec!["--urgency=critical".to_owned(), title, text]),
    ]
}

/// Files being written on threads of their own (exports, shares, Save As): since when, whether
/// from the artwork open (which takes no input meanwhile, so nothing the export would end is
/// started), and the thread, waited for before the app ends so none is cut short. And what they
/// had to say, shown on the window's thread (some systems show dialogs only from there).
type Writing = (std::time::Instant, bool, std::thread::JoinHandle<()>);
static WRITING: std::sync::Mutex<Vec<Writing>> = std::sync::Mutex::new(Vec::new());
static TOLD: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Runs `write` on a thread of its own, among those the app waits for as it ends; `from_canvas`
/// when it writes the artwork open.
pub fn write_meanwhile(from_canvas: bool, write: impl FnOnce() + Send + 'static) {
    let started = std::thread::spawn(write);
    let mut writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    writing.retain(|(.., w)| !w.is_finished());
    writing.push((std::time::Instant::now(), from_canvas, started));
}

/// Whether a file is still being written.
pub fn still_writing() -> bool {
    WRITING.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(.., w)| !w.is_finished())
}

/// Since when the artwork open has been written, if it still is.
pub fn exporting_since() -> Option<std::time::Instant> {
    let writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    writing.iter().filter(|(_, from_canvas, w)| *from_canvas && !w.is_finished()).map(|(since, ..)| *since).min()
}

/// Waits for the files still being written.
pub fn finish_writing() {
    let writing = std::mem::take(&mut *WRITING.lock().unwrap_or_else(|e| e.into_inner()));
    for (.., w) in writing {
        let _ = w.join();
    }
}

/// A message from a thread of its own (writing, reading), to show on the window's
/// ([`tell_what_was_told`]).
pub fn tell_later(title: &str, text: &str) {
    TOLD.lock().unwrap_or_else(|e| e.into_inner()).push((title.to_owned(), text.to_owned()));
}

/// Keeps `session`'s brushes in their folder. One that can't (the disk full, say) is told once,
/// from the window's thread, and not again until they are kept once more.
pub fn keep_brushes(session: &procraft_core::CanvasSession) {
    static FAILING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if session.keep_brushes(brushes_dir()) {
        FAILING.store(false, std::sync::atomic::Ordering::Relaxed);
    } else if !FAILING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tell_later(
            "Can't keep the brushes",
            "The brush library couldn't be kept. The disk may be full: brushes brought in or changed \
             since are lost when ProCraft closes, unless some room is made.",
        );
    }
}

/// Shows the messages the writing threads left.
pub fn tell_what_was_told() {
    let told = std::mem::take(&mut *TOLD.lock().unwrap_or_else(|e| e.into_inner()));
    for (title, text) in told {
        tell(&title, &text);
    }
}

/// Asks where to keep a file called `name` (ending `extension`, a `kind` of file) and writes
/// `bytes` there.
pub fn save_file(bytes: &[u8], name: &str, extension: &str, kind: &str) {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " _-".contains(c) { c } else { '_' }).collect();
    let safe = if safe.trim().is_empty() { "Brush".to_owned() } else { safe };
    let Some(path) = FileDialog::new()
        .set_title(format!("Share {kind}"))
        .add_filter(kind, &[extension])
        .set_file_name(format!("{safe}.{extension}"))
        .save_file()
    else {
        return;
    };
    // a file there already is replaced only once this one is written whole
    if let Err(e) = write_whole(&path, bytes) {
        tell("Can't share", &e.to_string());
    }
}

/// A sound the user picks (titled `title`): a WAV file, read whole, with its name.
pub fn pick_sound(title: &str) -> Option<(Vec<u8>, String)> {
    let path = FileDialog::new().set_title(title).add_filter("Sounds", &["wav", "wave"]).pick_file()?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::fs::read(&path) {
        Ok(bytes) => Some((bytes, name)),
        Err(_) => {
            say("This file could not be read.");
            None
        }
    }
}

/// A file the user picks (titled `title`), read whole, with its name.
pub fn pick_file(title: &str) -> Option<(Vec<u8>, String)> {
    let path = FileDialog::new().set_title(title).pick_file()?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::fs::read(&path) {
        Ok(bytes) => Some((bytes, name)),
        Err(_) => {
            say("This file could not be read.");
            None
        }
    }
}

/// Says the canvas holds no more layers, and how many it holds.
pub fn no_room(session: &CanvasSession) {
    let most = session.layer_stats().maximum;
    tell("No room", &format!("This canvas holds at most {most} layers."));
}

/// Where the brush library, the brushes' choices and their sliders are kept.
pub fn brushes_dir() -> String {
    app_dir().join("brushes").to_string_lossy().into_owned()
}

/// The gesture settings kept: a line for each switch (`EYEDROPPER.HOLD true`) and each page's
/// delay (`EYEDROPPER.delay 0.4`).
pub fn kept_gestures() -> KeptGestures {
    let mut kept = KeptGestures::default();
    let text = std::fs::read_to_string(app_dir().join("gestures")).unwrap_or_default();
    for line in text.lines() {
        let Some((name, value)) = line.split_once(' ') else { continue };
        if name.ends_with(".delay") {
            if let Ok(v) = value.parse() {
                kept.delays.insert(name.to_owned(), v);
            }
        } else if let Ok(v) = value.parse() {
            kept.switches.insert(name.to_owned(), v);
        }
    }
    kept
}

pub fn keep_gestures(kept: &KeptGestures) {
    let mut lines: Vec<String> = kept.switches.iter().map(|(name, on)| format!("{name} {on}")).collect();
    lines.extend(kept.delays.iter().map(|(name, s)| format!("{name} {s}")));
    lines.sort();
    keep_text("gestures", &(lines.join("\n") + "\n"));
}

/// The brush colour and the one masks paint with as they were last left (hue, saturation and
/// brightness, 0..1), if they were kept.
pub fn kept_colours() -> Option<([f32; 3], [f32; 3])> {
    let text = std::fs::read_to_string(app_dir().join("colour")).ok()?;
    let v: Vec<f32> = text
        .split_whitespace()
        .filter_map(|t| t.parse::<f32>().ok())
        .filter(|x| x.is_finite())
        .map(|x| x.clamp(0.0, 1.0))
        .collect();
    match v[..] {
        [h, s, b, mh, ms, mb] => Some(([h, s, b], [mh, ms, mb])),
        [h, s, b] => Some(([h, s, b], [0.0; 3])),
        _ => None,
    }
}

pub fn keep_colours(brush: [f32; 3], mask: [f32; 3]) {
    let [h, s, b] = brush;
    let [mh, ms, mb] = mask;
    keep_text("colour", &format!("{h} {s} {b} {mh} {ms} {mb}"));
}

/// The QuickMenu's buttons kept, as the core writes them.
pub fn kept_quick_menu() -> Option<String> {
    std::fs::read_to_string(app_dir().join("quick_menu")).ok().map(|s| s.trim().to_owned())
}

/// The QuickMenu's menus and which is active, as the core writes them (with none, the one kept
/// before there were several, by [`kept_quick_menu`]).
pub fn kept_quick_menus() -> (Option<String>, usize) {
    let menus = std::fs::read_to_string(app_dir().join("quick_menus")).ok();
    let active = std::fs::read_to_string(app_dir().join("quick_menu_active")).ok();
    (menus, active.and_then(|s| s.trim().parse().ok()).unwrap_or(0))
}

pub fn keep_quick_menus(saved: &str, active: usize) {
    keep_text("quick_menus", saved);
    keep_text("quick_menu_active", &active.to_string());
}

/// A small file of the app's settings written (a failure only logged: the setting holds this run).
pub fn keep_text(name: &str, text: &str) {
    let dir = app_dir();
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| write_whole(&dir.join(name), text.as_bytes())) {
        log::warn!("keeping {name}: {e}");
    }
}

/// Writes `bytes` to the app's file `path` whole or not at all: into a file beside it, then
/// renamed over it, so a write cut short leaves the last one.
pub fn write_whole(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let next = path.with_extension("next");
    std::fs::write(&next, bytes)?;
    std::fs::rename(&next, path)
}

/// A new artwork as `preset` says: its size, DPI, background and colour profile.
pub fn new_from_preset(preset: &CanvasPreset) -> Arc<CanvasSession> {
    let rgb = [16, 8, 0].map(|shift| ((preset.background >> shift) & 0xFF) as f32 / 255.0).to_vec();
    let (w, h) = (preset_width(preset.clone()), preset_height(preset.clone()));
    let session = CanvasSession::new_custom(w, h, preset.dpi, rgb, !preset.background_hidden, String::new());
    session.set_new_profile(preset.profile.clone());
    session
}

/// The time-lapse settings `preset` gives a new artwork, kept in its folder `dir` before it opens.
pub fn keep_preset_time_lapse(preset: &CanvasPreset, dir: &str) {
    procraft_core::TimeLapse::new(dir.to_owned()).set_settings(procraft_core::TimeLapseSettings {
        resolution: preset.time_lapse_resolution,
        quality: preset.time_lapse_quality,
        hevc: preset.time_lapse_hevc,
    });
}

/// A colour profile the user picks (an .icc file), added to those a canvas can be in; its name, or
/// none (said so) if it isn't one.
pub fn import_profile() -> Option<String> {
    let path = FileDialog::new().set_title("Import").add_filter("Color profiles", &["icc", "icm"]).pick_file()?;
    let name = std::fs::read(path).ok().and_then(procraft_core::import_profile);
    if name.is_none() {
        tell("Can't import", "This file isn't a color profile.");
    }
    name
}

/// Whether the file at `path` is a plugin, by its name.
pub fn is_plugin_file(path: &Path) -> bool {
    // (the old name too, from before the hyphen)
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("procraft-plugin") || e.eq_ignore_ascii_case("procraftplugin"))
}

/// A plugin file the user picks.
pub fn pick_plugin() -> Option<PathBuf> {
    FileDialog::new()
        .set_title("Import")
        .add_filter("ProCraft plugins", &["procraft-plugin", "procraftplugin"])
        .pick_file()
}

/// A picture the user picks (PNG, JPEG, WebP, GIF, BMP or TIFF), to make a new artwork of
/// ([`picture_artwork`]).
pub fn pick_picture() -> Option<PathBuf> {
    FileDialog::new()
        .set_title("Photo")
        .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"])
        .pick_file()
}

/// The picture at `path` as a new one-layer artwork its size, named after the file; why not if
/// it can't be read.
pub fn picture_artwork(path: &Path) -> Result<Arc<CanvasSession>, String> {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let picture = image::open(path).map_err(|e| e.to_string())?;
    let rgba = picture.to_rgba8();
    let image = ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
    Ok(CanvasSession::from_image(image, name))
}

/// What a model to paint on can be: an OBJ, a USD (packaged or not), or a zip holding one with
/// its files.
const MODEL_EXTENSIONS: [&str; 6] = ["obj", "usdz", "usdc", "usda", "usd", "zip"];

/// Whether `path` is a model's file (or a zip holding one), by its name.
pub fn is_model_file(path: &Path) -> bool {
    MODEL_EXTENSIONS.contains(&extension_of(path).as_str())
}

fn extension_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// What a palette's file can be: Swatches, Adobe Swatch Exchange and Photoshop's colour swatches.
const PALETTE_EXTENSIONS: [&str; 3] = ["swatches", "ase", "aco"];

/// Whether `path` is a palette's file, by its name.
pub fn is_palette_file(path: &Path) -> bool {
    PALETTE_EXTENSIONS.contains(&extension_of(path).as_str())
}

/// The files the gallery's Import brings in, as the user picks them: an artwork or a model (as
/// [`pick_artwork`] takes), palettes and brushes.
pub fn pick_import() -> Option<Vec<PathBuf>> {
    let brushes: Vec<&str> =
        ["brush", "brushset", "brushlibrary"].into_iter().chain(procraft_core::OTHER_BRUSH_EXTENSIONS).collect();
    let more: Vec<&str> = PALETTE_EXTENSIONS.into_iter().chain(brushes).collect();
    pick_artwork_files(&more)
}

/// Artworks, models and the files with extensions `more`, as the user picks them.
pub fn pick_artwork_files(more: &[&str]) -> Option<Vec<PathBuf>> {
    let artworks = [
        "procraft",
        "procreate",
        "psd",
        "psb",
        "pdf",
        "png",
        "obj",
        "usdz",
        "usdc",
        "usda",
        "usd",
        "zip",
        "mtl",
        "jpg",
        "jpeg",
        "webp",
        "gif",
        "bmp",
        "tif",
        "tiff",
    ];
    let all: Vec<&str> = artworks.iter().chain(more).copied().collect();
    FileDialog::new().set_title("Import").add_filter("Artworks and models", &all).pick_files()
}

/// The artwork in the files `paths` (picked, or dropped on the gallery): a layered package, a
/// PSD, a PDF, a picture as a one-layer artwork, or a model to paint on with the other files
/// beside it.
pub fn open_artwork_files(paths: Vec<PathBuf>) -> Option<(Arc<CanvasSession>, Option<PathBuf>)> {
    // a model among them takes the rest as the files picked with it
    if let Some(model) = paths.iter().find(|p| is_model_file(p)) {
        return open_model(model, &paths).map(|s| (s, None)).map_err(|e| tell_later("Can't open the model", &e)).ok();
    }
    let path = paths.into_iter().next()?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // a model's artwork opens from where it is unpacked, and is kept as any new artwork is
    if procraft_core::zip_file_kind(path.to_string_lossy().into_owned()) == procraft_core::ZipKind::ModelArtwork {
        let opened = open_model_artwork(&path).map(|s| (s, None));
        return opened.map_err(|e| tell_later("Can't open the model", &e)).ok();
    }
    let opened = match extension.as_str() {
        "psd" | "psb" => open_psd(&path, &name),
        "pdf" => open_pdf(&path, &name),
        "png" => read_png(&path).map(|image| CanvasSession::from_image(image, name)),
        "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => image::open(&path)
            .map(|picture| {
                let rgba = picture.to_rgba8();
                let image = ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
                CanvasSession::from_image(image, name)
            })
            .map_err(|e| e.to_string()),
        _ => CanvasSession::open_package(path.to_string_lossy().into_owned()).map_err(|e| e.to_string()),
    };
    // a package brings its time-lapse too
    let package = matches!(extension.as_str(), "procraft" | "procreate").then_some(path);
    opened.map(|s| (s, package)).map_err(|e| tell_later("Can't open the file", &e)).ok()
}

/// The model's artwork in file `path`, unpacked beside the other things opened from files.
fn open_model_artwork(path: &Path) -> Result<Arc<CanvasSession>, String> {
    let work = std::env::temp_dir().join(format!("procraft-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let dir = work.to_string_lossy().into_owned();
    if !procraft_core::unpack_model_artwork(path.to_string_lossy().into_owned(), dir.clone()) {
        return Err("This file can't be opened.".to_owned());
    }
    let opened = CanvasSession::open(dir).map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&work);
    opened
}

/// The PDF in file `path` as a new artwork named `name`: each page a layer ("Page 1" at the
/// bottom), drawn on white at 300 dpi and fitted to the first page's size, with Page Assist on.
fn open_pdf(path: &Path, name: &str) -> Result<Arc<CanvasSession>, String> {
    let work = std::env::temp_dir().join(format!("procraft-pdf-{}", std::process::id()));
    let (pages, artwork) = (work.join("pages"), work.join("artwork"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&pages).map_err(|e| e.to_string())?;
    let unreadable = || "This PDF can't be opened.".to_owned();
    let opened = crate::pdf::draw_pages(path, &pages)
        .map_err(|e| {
            log::error!("drawing a PDF's pages: {e}");
            if e.to_string() == crate::pdf::NO_POPPLER { crate::pdf::NO_POPPLER.to_owned() } else { unreadable() }
        })
        .and_then(|(count, w, h)| {
            let dir = artwork.to_string_lossy().into_owned();
            match import_pages(
                pages.to_string_lossy().into_owned(),
                count,
                w,
                h,
                crate::pdf::DPI,
                dir.clone(),
                name.to_owned(),
            ) {
                PagesOutcome::Imported => CanvasSession::open(dir).map_err(|e| e.to_string()),
                PagesOutcome::TooManyPages { most } => {
                    Err(format!("This PDF has more pages than a canvas its size can hold as layers ({most})."))
                }
                PagesOutcome::Unreadable => Err(unreadable()),
            }
        });
    // the session holds what it read
    let _ = std::fs::remove_dir_all(&work);
    opened
}

/// The model in file `path` to paint on, with the other files `picked` along with it; its own
/// textures in its texture sets.
fn open_model(path: &Path, picked: &[PathBuf]) -> Result<Arc<CanvasSession>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file_name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let files = picked
        .iter()
        .filter(|p| p.as_path() != path)
        .filter_map(|p| std::fs::read(p).ok().map(|bytes| ModelFile { name: file_name(p), bytes }))
        .collect();
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|n| !n.trim().is_empty());
    let name = name.unwrap_or_else(|| "Untitled Artwork".to_owned());
    let session = CanvasSession::open_model(bytes, file_name(path), files, name, 0).map_err(|e| e.to_string())?;
    session.load_model_textures();
    Ok(session)
}

/// A PSD made an artwork in a folder of its own, then opened from there.
fn open_psd(path: &Path, name: &str) -> Result<Arc<CanvasSession>, String> {
    let dir = std::env::temp_dir().join(format!("procraft-psd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let outcome = import_psd(path.to_string_lossy().into_owned(), dir.to_string_lossy().into_owned(), name.to_owned());
    let opened = match outcome {
        PsdOutcome::Imported => CanvasSession::open(dir.to_string_lossy().into_owned()).map_err(|e| e.to_string()),
        PsdOutcome::TooManyLayers { most } => Err(format!("It has more layers than a canvas its size holds ({most}).")),
        PsdOutcome::Unsupported => {
            Err("This kind of PSD isn't read (bitmap, indexed, Lab or 32 bits a channel).".into())
        }
        PsdOutcome::Unreadable => Err("It isn't a PSD, or it's damaged.".into()),
    };
    // the opened artwork holds its layers' files by now
    let _ = std::fs::remove_dir_all(&dir);
    opened
}

/// A picture's pixels (in `bytes`: a PNG, JPEG, WebP, GIF, BMP or TIFF) as 0xAARRGGBB, for its
/// colours; none if it isn't one.
pub fn picture_pixels(bytes: &[u8]) -> Option<Vec<u32>> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let argb = |&[r, g, b, a]: &[u8; 4]| u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    Some(rgba.as_raw().as_chunks::<4>().0.iter().map(argb).collect())
}

/// A PNG's size and pixels (in `bytes`), straight RGBA; none if it isn't one.
pub fn png_rgba(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = buf.get(..frame.buffer_size())?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        _ => return None,
    };
    Some((frame.width, frame.height, rgba))
}

/// A PNG's pixels, straight RGBA.
pub fn read_png(path: &Path) -> Result<ThumbnailImage, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("too large")?];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let bytes = buf.get(..frame.buffer_size()).ok_or("short image")?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::GrayscaleAlpha => bytes.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        other => return Err(format!("PNG colours not read: {other:?}")),
    };
    Ok(ThumbnailImage { width: frame.width, height: frame.height, rgba })
}

/// Shows the app's name and version.
pub fn about() {
    MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("ProCraft")
        .set_description(format!("Version {}", env!("PROCRAFT_VERSION")))
        .show();
}

/// Saves the artwork as a .procraft package, where it was last saved unless `ask` (or it never
/// was): written on a thread of its own (a large one takes seconds), the interface told when it
/// is ([`Interface::saved_as`]).
pub fn save(session: &Arc<CanvasSession>, ui: &mut Interface, ask: bool) {
    let path = match ui.path.clone().filter(|_| !ask) {
        Some(p) => p,
        None => {
            let Some(p) = FileDialog::new()
                .set_title("Save")
                .add_filter("ProCraft artwork", &["procraft"])
                .set_file_name(format!("{}.procraft", session.name()))
                .save_file()
            else {
                return;
            };
            with_extension(p, "procraft")
        }
    };
    let edits = session.edits();
    let time_lapse = ui.artwork_dir.as_deref().map(crate::timelapse::for_package);
    let (session, (done, saved)) = (Arc::clone(session), std::sync::mpsc::channel());
    write_meanwhile(true, move || {
        let written = session.export_package(path.to_string_lossy().into_owned(), seed(), time_lapse);
        let _ = done.send(written.map(|()| (path, edits)).map_err(|e| e.to_string()));
    });
    ui.saving = Some(saved);
}

/// Writes the artwork as `format` where the user picks, its card showing how far it has come;
/// cancelled, nothing is left there.
pub fn export(
    session: &Arc<CanvasSession>,
    format: Format,
    dir: Option<&str>,
    pdf_quality: u8,
    card: &mut crate::export_card::ExportCard,
) {
    if card.busy() {
        return;
    }
    // each layer (or a model's texture set) a PNG file, in a folder the user picks
    if matches!(format, Format::LayerPngs | Format::ChosenLayerPngs | Format::ModelTextures) {
        let title =
            if format == Format::ModelTextures { "Share textures as PNG files" } else { "Share layers as PNG files" };
        let Some(dir) = FileDialog::new().set_title(title).pick_folder() else { return };
        let dir = dir.to_string_lossy().into_owned();
        writing(session, card, "Can't share", move |session| {
            match format {
                Format::LayerPngs => session.export_layer_pngs(dir),
                Format::ModelTextures => session.export_model_textures(dir),
                _ => session.export_chosen_pngs(dir),
            }
            .map(|files| files.into_iter().map(PathBuf::from).collect())
        });
        return;
    }
    let Some(path) = FileDialog::new()
        .set_title(format!("Export {}", format.name()))
        .add_filter(format.name(), &[format.extension()])
        .set_file_name(format!("{}.{}", export_file_name(session.name(), format.package()), format.extension()))
        .save_file()
    else {
        return;
    };
    let path = with_extension(path, format.extension());
    let artwork = dir.map(str::to_owned);
    let time_lapse = dir.map(crate::timelapse::for_package);
    writing(session, card, "Can't export", move |session| {
        // written beside it first, so a cancelled export leaves what was there as it was
        let part = PathBuf::from(format!("{}.part", path.to_string_lossy()));
        let at = part.to_string_lossy().into_owned();
        match format {
            Format::ModelArtwork => session.export_model_artwork(artwork.unwrap_or_default(), at),
            Format::Procraft | Format::Package => session.export_package(at, seed(), time_lapse),
            Format::Psd => session.export_psd(at),
            Format::Pdf => session.export_pdf(at, false, pdf_quality),
            Format::LayersPdf => session.export_pdf(at, true, pdf_quality),
            Format::Png => session.export_png(at),
            Format::Tiff => session.export_tiff(at),
            Format::Jpeg => session.export_jpeg(at),
            Format::ModelJpeg => session.export_model_jpeg(at),
            Format::ModelObj => session.export_model_obj(at),
            Format::ModelUsdz => session.export_model_usdz(at),
            Format::ModelRender => session.export_model_render(at, false),
            Format::ModelTiff => session.export_model_tiff(at),
            Format::Svg => session.export_svg(at),
            Format::LayerPngs | Format::ChosenLayerPngs | Format::ModelTextures => Ok(()),
        }?;
        Ok(vec![part])
    });
}

/// Writes the file plugin export `key` makes of the artwork's layers where the user picks, as
/// [`export`] writes the app's own.
pub fn export_plugin(session: &Arc<CanvasSession>, key: &str, card: &mut crate::export_card::ExportCard) {
    if card.busy() {
        return;
    }
    let Some(export) = session.plugin_exports().into_iter().find(|e| e.key == key) else { return };
    let extension = export.extension.clone();
    let Some(path) = FileDialog::new()
        .set_title(format!("Export {}", export.label))
        .add_filter(export.label.as_str(), &[extension.as_str()])
        .set_file_name(format!("{}.{extension}", export_file_name(session.name(), false)))
        .save_file()
    else {
        return;
    };
    let path = with_extension(path, &extension);
    let key = key.to_owned();
    writing(session, card, "Can't export", move |session| {
        // written beside it first, so a cancelled export leaves what was there as it was
        let part = PathBuf::from(format!("{}.part", path.to_string_lossy()));
        session.export_plugin(key, part.to_string_lossy().into_owned())?;
        Ok(vec![part])
    });
}

/// Writes an export on a thread of its own (a large one takes seconds, the window drawing on
/// meanwhile, as the tablet's does), `card` showing how far it has come; telling `failed` with the
/// reason if it can't. `write` returns what it wrote: a file ending ".part" is put in place of the
/// one it was written beside once made; cancelled, everything it wrote goes.
fn writing(
    session: &Arc<CanvasSession>,
    card: &mut crate::export_card::ExportCard,
    failed: &'static str,
    write: impl FnOnce(&CanvasSession) -> Result<Vec<PathBuf>, procraft_core::MobileError> + Send + 'static,
) {
    session.reset_export_progress();
    let following = Arc::clone(session);
    let exporting = card.start(move || following.export_progress());
    let session = Arc::clone(session);
    write_meanwhile(true, move || {
        let written = write(&session);
        let placed = written.map_err(|e| e.to_string()).and_then(|files| place_written(&files, exporting.cancelled()));
        match placed {
            Ok(made) => exporting.ended(made),
            Err(e) => {
                exporting.ended(false);
                if !exporting.cancelled() {
                    tell_later(failed, &e);
                }
            }
        }
    });
}

/// What an export wrote, put where it goes: a file ending ".part" moved over the one it was
/// written beside (others, as a folder's PNGs, are where they go already); `cancelled`, everything
/// it wrote removed instead, so what was there stays as it was. Whether it was put in place.
fn place_written(files: &[PathBuf], cancelled: bool) -> Result<bool, String> {
    if cancelled {
        for f in files {
            let _ = std::fs::remove_file(f);
        }
        return Ok(false);
    }
    for f in files {
        let name = f.to_string_lossy();
        if let Some(target) = name.strip_suffix(".part") {
            std::fs::rename(f, target).map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

/// Artworks from the gallery written as `format`: one where the user picks, several into a folder
/// the user picks, each named by its name (a number after it when two share one). Each is opened
/// on a GPU device of its own, never shown. `card` shows them made, an artwork at a time;
/// cancelled, none is left where it was going.
pub fn share_artworks(
    list: &[procraft_core::GalleryArtwork],
    format: Format,
    pdf_quality: u8,
    card: &mut crate::export_card::ExportCard,
) {
    if card.busy() {
        return;
    }
    let extension = format.extension();
    let file_name = |name: &str| export_file_name(name.to_owned(), false);
    let targets: Vec<(procraft_core::GalleryArtwork, PathBuf)> = if let [one] = list {
        let Some(path) = FileDialog::new()
            .set_title(format!("Share {}", format.name()))
            .add_filter(format.name(), &[extension])
            .set_file_name(format!("{}.{extension}", file_name(&one.name)))
            .save_file()
        else {
            return;
        };
        vec![(one.clone(), with_extension(path, extension))]
    } else {
        let Some(dir) = FileDialog::new().set_title(format!("Share as {}", format.name())).pick_folder() else {
            return;
        };
        let mut used = std::collections::HashSet::new();
        list.iter()
            .map(|a| {
                let base = file_name(&a.name);
                let (mut name, mut n) = (base.clone(), 2);
                while !used.insert(name.clone()) {
                    name = format!("{base} {n}");
                    n += 1;
                }
                (a.clone(), dir.join(format!("{name}.{extension}")))
            })
            .collect()
    };
    // written on a thread of their own (each opened on a device of its own takes seconds), the
    // window drawing on meanwhile; each beside where it goes until all are made
    let done = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let following = Arc::clone(&done);
    let exporting = card.start(move || f32::from_bits(following.load(std::sync::atomic::Ordering::Relaxed)));
    let count = targets.len().max(1);
    write_meanwhile(false, move || {
        let mut written = Vec::new();
        for (i, (a, path)) in targets.into_iter().enumerate() {
            if exporting.cancelled() {
                break;
            }
            let part = PathBuf::from(format!("{}.part", path.to_string_lossy()));
            if let Err(e) = write_artwork(&a, format, &part.to_string_lossy(), pdf_quality) {
                // those made before it are kept, as before
                let _ = place_written(&written, false);
                exporting.ended(false);
                tell_later("Can't share", &format!("{}: {e}", a.name));
                return;
            }
            written.push(part);
            let progress = (i + 1) as f32 / count as f32;
            done.store(progress.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
        match place_written(&written, exporting.cancelled()) {
            Ok(made) => exporting.ended(made),
            Err(e) => {
                exporting.ended(false);
                tell_later("Can't share", &e);
            }
        }
    });
}

/// The artwork `a` from the gallery written as `format` at `path` (a PDF's pictures at
/// `pdf_quality`), opened on a GPU device of its own, never shown.
pub fn write_artwork(
    a: &procraft_core::GalleryArtwork,
    format: Format,
    path: &str,
    pdf_quality: u8,
) -> Result<(), String> {
    let path = path.to_owned();
    let s = CanvasSession::open(a.dir.clone()).map_err(|e| e.to_string())?;
    if !s.prepare_offscreen() {
        return Err("There's no GPU device to draw it with.".to_owned());
    }
    match format {
        Format::Procraft | Format::Package => {
            s.export_package(path, seed(), Some(crate::timelapse::for_package(&a.dir)))
        }
        Format::Psd => s.export_psd(path),
        Format::Pdf | Format::LayersPdf => s.export_pdf(path, false, pdf_quality),
        Format::Tiff => s.export_tiff(path),
        Format::Jpeg => s.export_jpeg(path),
        _ => s.export_png(path),
    }
    .map_err(|e| e.to_string())
}

/// What an artwork dragged out of the gallery becomes, by Drag and Drop export's preferred file
/// format ([`crate::prefs::DRAG_FORMATS`]).
pub const DRAG_FORMATS: [Format; 5] = [Format::Procraft, Format::Psd, Format::Jpeg, Format::Png, Format::Tiff];

/// An artwork lifted out of the gallery (the folder `dir`, named `name`) written as `format` into
/// the system's temporary folder for dragging; its path.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn drag_file(a: &procraft_core::GalleryArtwork, format: Format) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("ProCraft drags");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = export_file_name(a.name.clone(), false);
    let path = dir.join(format!("{name}.{}", format.extension()));
    write_artwork(a, format, &path.to_string_lossy(), 100)?;
    Ok(path)
}

/// A dragged artwork's picture: its thumbnail fitted in `side` pixels, on white, as a PNG.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn drag_picture(dir: &str, side: u32) -> Option<Vec<u8>> {
    let thumbnail = image::open(Path::new(dir).join("thumbnail.png")).ok()?.thumbnail(side, side).to_rgba8();
    let mut on_white =
        image::RgbaImage::from_pixel(thumbnail.width(), thumbnail.height(), image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut on_white, &thumbnail, 0, 0);
    let mut png = Vec::new();
    on_white.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// `path`, ending in `.extension`.
fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)) {
        path
    } else {
        path.with_extension(extension)
    }
}

/// A new number for a package's layer ids.
fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_artwork_records_its_time_lapse_as_its_preset_says() {
        let dir = folder("preset-time-lapse");
        let mut preset = procraft_core::standard_presets().remove(0);
        (preset.time_lapse_resolution, preset.time_lapse_quality, preset.time_lapse_hevc) = (2, 3, true);
        keep_preset_time_lapse(&preset, &dir.to_string_lossy());
        let settings = procraft_core::TimeLapse::new(dir.to_string_lossy().into_owned()).settings();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(settings, procraft_core::TimeLapseSettings { resolution: 2, quality: 3, hevc: true });
    }

    #[test]
    fn a_message_has_other_ways_of_showing_with_its_words_in_each() {
        let ways = other_dialogs("No drawing", "no GPU");
        assert_eq!(ways.map(|(program, _)| program), ["kdialog", "xmessage", "notify-send"]);
        for (_, args) in other_dialogs("No drawing", "no GPU") {
            assert!(args.iter().any(|a| a.contains("no GPU")), "{args:?}");
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-place-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_made_export_replaces_the_file_it_was_written_beside() {
        let dir = folder("made");
        let (target, part) = (dir.join("art.psd"), dir.join("art.psd.part"));
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        assert_eq!(place_written(std::slice::from_ref(&part), false), Ok(true));
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_export_leaves_what_was_there_as_it_was() {
        let dir = folder("cancelled");
        let (target, part) = (dir.join("art.psd"), dir.join("art.psd.part"));
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();
        // a folder's PNGs, written where they go: removed too
        let png = dir.join("Layer 1.png");
        std::fs::write(&png, b"png").unwrap();
        assert_eq!(place_written(&[part.clone(), png.clone()], true), Ok(false));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!part.exists() && !png.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
