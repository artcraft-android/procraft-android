//! Artworks on disk: new, open (ProCraft packages, PSDs and PNGs), save as .procraft, export,
//! and asking before unsaved changes are lost.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_core::{
    CanvasPreset, CanvasSession, KeptGestures, PsdOutcome, ThumbnailImage, import_psd, preset_height, preset_width,
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
            Format::Jpeg => "JPEG",
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
            Format::Jpeg => "jpg",
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
    keep_text("gestures", &(lines.join("\n") + "\n"));
}

/// The QuickMenu's buttons kept, as the core writes them.
pub fn kept_quick_menu() -> Option<String> {
    std::fs::read_to_string(app_dir().join("quick_menu")).ok().map(|s| s.trim().to_owned())
}

pub fn keep_quick_menu(saved: &str) {
    keep_text("quick_menu", saved);
}

/// A small file of the app's settings written (a failure only logged: the setting holds this run).
pub fn keep_text(name: &str, text: &str) {
    let dir = app_dir();
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(name), text)) {
        log::warn!("keeping {name}: {e}");
    }
}

/// A new artwork as `preset` says: its size, DPI, background and colour profile.
pub fn new_from_preset(preset: &CanvasPreset) -> Arc<CanvasSession> {
    let rgb = [16, 8, 0].map(|shift| ((preset.background >> shift) & 0xFF) as f32 / 255.0).to_vec();
    let (w, h) = (preset_width(preset.clone()), preset_height(preset.clone()));
    let session = CanvasSession::new_custom(w, h, preset.dpi, rgb, !preset.background_hidden, String::new());
    session.set_new_profile(preset.profile.clone());
    session
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

/// A picture the user picks (PNG, JPEG, WebP, GIF, BMP or TIFF) as a new one-layer artwork its
/// size.
pub fn pick_picture() -> Option<Arc<CanvasSession>> {
    let path = FileDialog::new()
        .set_title("Photo")
        .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"])
        .pick_file()?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    match image::open(&path) {
        Ok(picture) => {
            let rgba = picture.to_rgba8();
            let image = ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
            Some(CanvasSession::from_image(image, name))
        }
        Err(e) => {
            tell("Can't open the picture", &e.to_string());
            None
        }
    }
}

/// An artwork the user picks: a layered package, a PSD, or a PNG as a one-layer artwork.
pub fn pick_artwork() -> Option<(Arc<CanvasSession>, Option<PathBuf>)> {
    let path = FileDialog::new()
        .set_title("Import")
        .add_filter("Artworks", &["procraft", "procreate", "psd", "psb", "png"])
        .pick_file()?;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let opened = match extension.as_str() {
        "psd" | "psb" => open_psd(&path, &name),
        "png" => read_png(&path).map(|image| CanvasSession::from_image(image, name)),
        _ => CanvasSession::open_package(path.to_string_lossy().into_owned()).map_err(|e| e.to_string()),
    };
    // a package brings its time-lapse too
    let package = matches!(extension.as_str(), "procraft" | "procreate").then_some(path);
    opened.map(|s| (s, package)).map_err(|e| tell("Can't open the file", &e)).ok()
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
    let time_lapse = ui.artwork_dir.as_deref().map(crate::timelapse::for_package);
    match session.export_package(path.to_string_lossy().into_owned(), seed(), time_lapse) {
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
pub fn export(session: &CanvasSession, format: Format, dir: Option<&str>) {
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
        Format::Procraft | Format::Package => {
            session.export_package(path, seed(), dir.map(crate::timelapse::for_package))
        }
        Format::Psd => session.export_psd(path),
        Format::Pdf => session.export_pdf(path, false, 100),
        Format::LayersPdf => session.export_pdf(path, true, 100),
        Format::Png => session.export_png(path),
        Format::Tiff => session.export_tiff(path),
        Format::Jpeg => {
            if let Err(e) = export_jpeg(session, &path) {
                tell("Can't export", &e);
            }
            return;
        }
        Format::LayerPngs | Format::ChosenLayerPngs => return,
    };
    if let Err(e) = written {
        tell("Can't export", &e.to_string());
    }
}

/// Artworks from the gallery written as `format`: one where the user picks, several into a folder
/// the user picks, each named by its name (a number after it when two share one). Each is opened
/// on a GPU device of its own, never shown.
pub fn share_artworks(list: &[procraft_core::GalleryArtwork], format: Format) {
    let extension = format.extension();
    let file_name = |name: &str| {
        let kept: String = name.chars().filter(|c| c.is_alphanumeric() || " _-".contains(*c)).collect();
        if kept.trim().is_empty() { "Untitled Artwork".to_owned() } else { kept.trim().to_owned() }
    };
    let targets: Vec<(&procraft_core::GalleryArtwork, PathBuf)> = if let [one] = list {
        let Some(path) = FileDialog::new()
            .set_title(format!("Share {}", format.name()))
            .add_filter(format.name(), &[extension])
            .set_file_name(format!("{}.{extension}", file_name(&one.name)))
            .save_file()
        else {
            return;
        };
        vec![(one, with_extension(path, extension))]
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
                (a, dir.join(format!("{name}.{extension}")))
            })
            .collect()
    };
    for (a, path) in targets {
        let path = path.to_string_lossy().into_owned();
        let written = CanvasSession::open(a.dir.clone()).map_err(|e| e.to_string()).and_then(|s| {
            if !s.prepare_offscreen() {
                return Err("There's no GPU device to draw it with.".to_owned());
            }
            match format {
                Format::Procraft | Format::Package => {
                    s.export_package(path, seed(), Some(crate::timelapse::for_package(&a.dir)))
                }
                Format::Psd => s.export_psd(path),
                Format::Pdf | Format::LayersPdf => s.export_pdf(path, false, 100),
                Format::Tiff => s.export_tiff(path),
                Format::Jpeg => return export_jpeg(&s, &path),
                _ => s.export_png(path),
            }
            .map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            tell("Can't share", &format!("{}: {e}", a.name));
            return;
        }
    }
}

/// The artwork as a JPEG at `path`: its picture written as a PNG beside it, then flattened onto
/// white.
fn export_jpeg(session: &CanvasSession, path: &str) -> Result<(), String> {
    let png = format!("{path}.png");
    session.export_png(png.clone()).map_err(|e| e.to_string())?;
    let picture = image::open(&png).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&png);
    let rgba = picture?.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (out, px) in rgb.pixels_mut().zip(rgba.pixels()) {
        let a = u16::from(px.0[3]);
        let over = |c: u8| ((u16::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        *out = image::Rgb([over(px.0[0]), over(px.0[1]), over(px.0[2])]);
    }
    rgb.save_with_format(path, image::ImageFormat::Jpeg).map_err(|e| e.to_string())
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
