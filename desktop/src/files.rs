//! Artworks on disk: new, open (ProCraft packages, PSDs and PNGs), save as .procraft, export,
//! and asking before unsaved changes are lost.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_core::{CanvasSession, KeptGestures, PsdOutcome, ThumbnailImage, import_psd};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

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
    Tiff,
    /// Each layer a page, or a PNG file in a folder.
    LayersPdf,
    LayerPngs,
    /// The current and picked layers, each a PNG file in a folder.
    ChosenLayerPngs,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Procraft => "ProCraft",
            Format::Package => "Procreate",
            Format::Psd => "PSD",
            Format::Pdf | Format::LayersPdf => "PDF",
            Format::Png | Format::LayerPngs | Format::ChosenLayerPngs => "PNG",
            Format::Tiff => "TIFF",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Procraft => "procraft",
            Format::Package => "procreate",
            Format::Psd => "psd",
            Format::Pdf | Format::LayersPdf => "pdf",
            Format::Png | Format::LayerPngs | Format::ChosenLayerPngs => "png",
            Format::Tiff => "tiff",
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

/// Shows a message with an OK button.
pub fn tell(title: &str, text: &str) {
    MessageDialog::new().set_level(MessageLevel::Error).set_title(title).set_description(text).show();
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
    if let Err(e) = std::fs::write(&path, bytes) {
        tell("Can't share", &e.to_string());
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
    keep_text(
        "gestures",
        &(lines.join(
            "
",
        ) + "
"),
    );
}

/// The QuickMenu's buttons kept, as the core writes them.
pub fn kept_quick_menu() -> Option<String> {
    std::fs::read_to_string(app_dir().join("quick_menu")).ok().map(|s| s.trim().to_owned())
}

pub fn keep_quick_menu(saved: &str) {
    keep_text("quick_menu", saved);
}

/// A small file of the app's settings written (a failure only logged: the setting holds this run).
fn keep_text(name: &str, text: &str) {
    let dir = app_dir();
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(name), text)) {
        log::warn!("keeping {name}: {e}");
    }
}

/// Whether the artwork may be let go of: nothing unsaved, or the user saved it or chose not to.
pub fn may_close(session: &CanvasSession, ui: &mut Interface) -> bool {
    if session.edits() == ui.saved_edits {
        return true;
    }
    let answer = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("ProCraft")
        .set_description(format!("Save the changes to “{}”?", session.name()))
        .set_buttons(MessageButtons::YesNoCancel)
        .show();
    match answer {
        MessageDialogResult::Yes => save(session, ui, false),
        MessageDialogResult::No => true,
        _ => false,
    }
}

/// A new artwork the size of the one open.
pub fn new_artwork(session: &CanvasSession, ui: &mut Interface) -> Option<Arc<CanvasSession>> {
    if !may_close(session, ui) {
        return None;
    }
    let info = session.info();
    let next = CanvasSession::new(info.width, info.height);
    ui.path = None;
    ui.saved_edits = next.edits();
    Some(next)
}

/// An artwork the user picks: a layered package, a PSD, or a PNG as a one-layer artwork.
pub fn open(session: &CanvasSession, ui: &mut Interface) -> Option<Arc<CanvasSession>> {
    if !may_close(session, ui) {
        return None;
    }
    let path = FileDialog::new()
        .set_title("Open")
        .add_filter("Artworks", &["procraft", "procreate", "psd", "psb", "png"])
        .pick_file()?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let opened = match extension.as_str() {
        "psd" | "psb" => open_psd(&path, &name),
        "png" => read_png(&path).map(|image| CanvasSession::from_image(image, name)),
        _ => CanvasSession::open_package(path.to_string_lossy().into_owned()).map_err(|e| e.to_string()),
    };
    match opened {
        Ok(next) => {
            // only a package is saved back where it came from
            ui.path = matches!(extension.as_str(), "procraft").then_some(path);
            ui.saved_edits = next.edits();
            Some(next)
        }
        Err(e) => {
            tell("Can't open the file", &e);
            None
        }
    }
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

/// A PNG's pixels (in `bytes`) as 0xAARRGGBB, for its colours; none if it isn't one.
pub fn png_pixels(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = buf.get(..frame.buffer_size())?;
    let argb = |[r, g, b, a]: [u8; 4]| u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    Some(match frame.color_type {
        png::ColorType::Rgba => data.as_chunks::<4>().0.iter().map(|&p| argb(p)).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().map(|&[g, a]| argb([g, g, g, a])).collect(),
        _ => return None,
    })
}

/// A PNG's pixels, straight RGBA.
fn read_png(path: &Path) -> Result<ThumbnailImage, String> {
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

/// A picture the user picks (a PNG) put on a new layer of its own.
pub fn insert_file(session: &CanvasSession) {
    let Some(path) = FileDialog::new().set_title("Insert a file").add_filter("Pictures", &["png"]).pick_file() else {
        return;
    };
    match read_png(&path) {
        Ok(image) => {
            session.insert_image(image, false);
        }
        Err(e) => tell("Can't insert the file", &e),
    }
}

/// Shows the app's name and version.
pub fn about() {
    MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("ProCraft")
        .set_description(format!("Version {}", env!("CARGO_PKG_VERSION")))
        .show();
}

/// Saves the artwork as a .procraft package, where it was last saved unless `ask` (or it never
/// was). True if it was saved.
pub fn save(session: &CanvasSession, ui: &mut Interface, ask: bool) -> bool {
    let path = match ui.path.clone().filter(|_| !ask) {
        Some(p) => p,
        None => {
            let Some(p) = FileDialog::new()
                .set_title("Save")
                .add_filter("ProCraft artwork", &["procraft"])
                .set_file_name(format!("{}.procraft", session.name()))
                .save_file()
            else {
                return false;
            };
            with_extension(p, "procraft")
        }
    };
    let edits = session.edits();
    match session.export_package(path.to_string_lossy().into_owned(), seed(), None) {
        Ok(()) => {
            ui.path = Some(path);
            ui.saved_edits = edits;
            true
        }
        Err(e) => {
            tell("Can't save", &e.to_string());
            false
        }
    }
}

/// Writes the artwork as `format` where the user picks.
pub fn export(session: &CanvasSession, format: Format) {
    // each layer a PNG file, in a folder the user picks
    if matches!(format, Format::LayerPngs | Format::ChosenLayerPngs) {
        let Some(dir) = FileDialog::new().set_title("Share layers as PNG files").pick_folder() else { return };
        let dir = dir.to_string_lossy().into_owned();
        let written = if format == Format::LayerPngs {
            session.export_layer_pngs(dir).map(drop)
        } else {
            session.export_chosen_pngs(dir).map(drop)
        };
        if let Err(e) = written {
            tell("Can't share", &e.to_string());
        }
        return;
    }
    let Some(path) = FileDialog::new()
        .set_title(format!("Export {}", format.name()))
        .add_filter(format.name(), &[format.extension()])
        .set_file_name(format!("{}.{}", session.name(), format.extension()))
        .save_file()
    else {
        return;
    };
    let path = with_extension(path, format.extension()).to_string_lossy().into_owned();
    let written = match format {
        Format::Procraft | Format::Package => session.export_package(path, seed(), None),
        Format::Psd => session.export_psd(path),
        Format::Pdf => session.export_pdf(path, false, 100),
        Format::LayersPdf => session.export_pdf(path, true, 100),
        Format::Png => session.export_png(path),
        Format::Tiff => session.export_tiff(path),
        Format::LayerPngs | Format::ChosenLayerPngs => return,
    };
    if let Err(e) = written {
        tell("Can't export", &e.to_string());
    }
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
