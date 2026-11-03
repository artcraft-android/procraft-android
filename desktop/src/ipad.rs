//! Artworks and brushes brought in from an iPad over USB: what Procreate keeps in its shared
//! Documents folder (On My iPad › Procreate in the Files app, where Share → Save to Files puts an
//! artwork, and its Inbox, where AirDrop leaves what it brings). Procreate's gallery itself isn't
//! shared with a computer; an artwork has to be saved to that folder first.
//!
//! Read with the system's libimobiledevice, as the videos are made with its ffmpeg: `afcclient`
//! lists and copies (a command of its own each time, so a cable pulled out only fails the one
//! under way), and, where `ifuse` is installed, a read-only mount gives the artworks' previews (a
//! preview is read out of the middle of an artwork, which `afcclient` can't do). The mount is only
//! there while the iPad's files show, read on threads of their own and let go lazily, so a device
//! that goes away never holds the app up.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use egui::{Align2, Color32, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};

use crate::look::{Look, Weight, argb, pc, text};

/// Procreate's app, whose Documents folder is read.
const PROCREATE: &str = "au.com.savageinteractive.procreate";
/// What the browser lists: the folders looked in (as the device names them), the kinds of file
/// brought in from there, its title and a word on where they come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Artworks (Procreate's own and PSDs): the folder Share → Save to Files puts them in, and
    /// AirDrop's Inbox.
    Artworks,
    /// The fonts Procreate has had imported.
    Fonts,
}

impl Kind {
    fn folders(self) -> &'static [&'static str] {
        match self {
            Kind::Artworks => &["/Documents", "/Documents/Inbox"],
            Kind::Fonts => &["/Documents/Fonts"],
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Kind::Artworks => &["procreate", "psd"],
            Kind::Fonts => &["ttf", "otf", "ttc"],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::Artworks => "iPad · Procreate",
            Kind::Fonts => "iPad · Procreate's fonts",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Kind::Artworks => "Artworks saved to On My iPad › Procreate in Files, and what AirDrop brought",
            Kind::Fonts => "The fonts imported into Procreate",
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Kind::Artworks => {
                "Nothing to bring in yet. In Procreate, Share an artwork and choose Save to Files → On My iPad → Procreate."
            }
            Kind::Fonts => "Procreate has no imported fonts on this iPad.",
        }
    }
}
/// The tools needed, and what each is for.
const TOOLS: [(&str, &str); 2] = [("idevice_id", "finding the iPad"), ("afcclient", "reading its files")];
/// Where usbmuxd, which reaches iPhones and iPads over USB, listens.
const USBMUXD: &str = "/var/run/usbmuxd";
/// How often whether an iPad is plugged in is looked at again.
const WATCH_EVERY: Duration = Duration::from_secs(3);
/// An artwork's preview inside it.
const PREVIEW: &str = "QuickLook/Thumbnail.png";

/// The iPad plugged in now (its UDID), as the watcher last saw.
static CONNECTED: Mutex<Option<String>> = Mutex::new(None);

/// A file on the iPad that can be brought in.
#[derive(Clone, Debug, PartialEq)]
pub struct IpadFile {
    /// Where it is on the device, its name, and its size in bytes.
    pub path: String,
    pub name: String,
    pub size: u64,
}

/// Starts looking, every few seconds, whether an iPad is plugged in (if the tools are there to
/// tell): what the gallery's Import offers follows it.
pub fn watch() {
    if !cfg!(target_os = "linux") || !on_path("idevice_id") {
        return;
    }
    let started = std::thread::Builder::new().name("ipad watch".into()).spawn(|| {
        loop {
            let found = run("idevice_id", &["-l"])
                .ok()
                .and_then(|out| out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_owned));
            if let Ok(mut c) = CONNECTED.lock() {
                *c = found;
            }
            std::thread::sleep(WATCH_EVERY);
        }
    });
    if let Err(e) = started {
        log::warn!("not watching for an iPad: {e}");
    }
}

/// Whether an iPad is plugged in, as last seen.
pub fn connected() -> bool {
    CONNECTED.lock().is_ok_and(|c| c.is_some())
}

/// The iPad to read, or what stands in the way, said so that it can be put right.
pub fn ready() -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("Importing from an iPad needs libimobiledevice, on Linux.".into());
    }
    let missing: Vec<String> =
        TOOLS.iter().filter(|(tool, _)| !on_path(tool)).map(|(tool, what)| format!("{tool} ({what})")).collect();
    if !missing.is_empty() {
        return Err(format!(
            "Install libimobiledevice: {} isn't on the PATH. On NixOS add libimobiledevice to your packages; \
             elsewhere, install the libimobiledevice-utils (or libimobiledevice) package.",
            missing.join(" and ")
        ));
    }
    if !usbmuxd_running() {
        return Err("usbmuxd isn't running, so the iPad can't be reached over USB. On NixOS set \
                    services.usbmuxd.enable = true; elsewhere install usbmuxd and start it \
                    (sudo systemctl start usbmuxd)."
            .into());
    }
    let out = run("idevice_id", &["-l"])?;
    let udid = out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_owned);
    udid.ok_or_else(|| "No iPad found. Connect it with a cable, unlock it, and tap Trust if it asks.".into())
}

/// The files of `kind` Procreate's folder holds, on the device `udid`.
pub fn list(udid: &str, kind: Kind) -> Result<Vec<IpadFile>, String> {
    let mut files = Vec::new();
    for (i, folder) in kind.folders().iter().enumerate() {
        match afc(udid, &["ls", "-l", folder]) {
            Ok(out) => files.extend(parse_listing(folder, &out, kind.extensions())),
            // the Inbox (or Fonts) is only there once something was put in it
            Err(_) if i > 0 || kind == Kind::Fonts => {}
            Err(e) => return Err(e),
        }
    }
    files.sort_by_key(|f| f.name.to_lowercase());
    Ok(files)
}

/// `file` copied off the device into `dir`; where it went. One that didn't come over whole (the
/// cable pulled out) is taken away again.
pub fn fetch(udid: &str, file: &IpadFile, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't make {}: {e}", dir.display()))?;
    let to = dir.join(&file.name);
    let local = to.to_string_lossy().into_owned();
    let got = afc(udid, &["get", &file.path, &local]);
    let whole = std::fs::metadata(&to).is_ok_and(|m| m.len() == file.size);
    if got.is_ok() && whole {
        return Ok(to);
    }
    let _ = std::fs::remove_file(&to);
    Err(match got {
        Err(e) => format!("{} didn't come over: {e}", file.name),
        Ok(_) => format!("{} didn't come over whole (was the iPad unplugged?).", file.name),
    })
}

/// The file at `local` copied onto the device `udid`, into Procreate's folder (On My iPad ›
/// Procreate in the Files app, for Procreate to import from), under a name not taken there; the
/// name it went by.
pub fn send(udid: &str, local: &Path) -> Result<String, String> {
    let taken: Vec<String> = afc(udid, &["ls", "/Documents"])
        .map(|out| out.lines().map(|l| strip_ansi(l).trim().to_owned()).collect())
        .unwrap_or_default();
    let name = free_name(&local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), &taken);
    let to = format!("/Documents/{name}");
    afc(udid, &["put", &local.to_string_lossy(), &to])?;
    // there whole, or not at all
    let size = std::fs::metadata(local).map(|m| m.len()).unwrap_or(0);
    let there = list(udid, Kind::Artworks)
        .ok()
        .and_then(|files| files.into_iter().find(|f| f.name == name).map(|f| f.size))
        .or_else(|| afc(udid, &["info", &to]).ok().and_then(|out| info_size(&out)));
    if there == Some(size) {
        Ok(name)
    } else {
        let _ = afc(udid, &["rm", &to]);
        Err(format!("{name} didn't reach the iPad whole (was it unplugged?)."))
    }
}

/// `name`, or with a number before its extension where it is taken already ("Art 2.procreate").
fn free_name(name: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_owned();
    }
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| format!("{stem} {n}{ext}")).find(|n| !taken.contains(n)).unwrap_or_else(|| name.to_owned())
}

/// The size in afcclient's `info` of a file.
fn info_size(out: &str) -> Option<u64> {
    out.lines().find_map(|l| l.trim().strip_prefix("\"st_size\":")?.trim().trim_end_matches(',').parse().ok())
}

/// The lines of an `ls -l` of `folder` that are files to bring in: permissions, links, owner,
/// group, size, day, month, year, time, then the name (which may hold spaces).
fn parse_listing(folder: &str, out: &str, extensions: &[&str]) -> Vec<IpadFile> {
    out.lines()
        .filter_map(|line| {
            let line = strip_ansi(line);
            // the prompt may come before the first line
            let line = line.rsplit("> ").next().unwrap_or(&line).trim_end().to_owned();
            if !line.starts_with('-') {
                return None;
            }
            let mut rest = line.as_str();
            let mut fields = Vec::new();
            for _ in 0..9 {
                let trimmed = rest.trim_start();
                let end = trimmed.find(' ')?;
                fields.push(trimmed.get(..end)?);
                rest = trimmed.get(end..)?;
            }
            let name = rest.trim_start().to_owned();
            let size = fields.get(4)?.parse().ok()?;
            let kind = Path::new(&name).extension()?.to_string_lossy().to_lowercase();
            extensions.contains(&kind.as_str()).then(|| IpadFile { path: format!("{folder}/{name}"), name, size })
        })
        .collect()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // ESC [ … letter
        if chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

/// afcclient run on Procreate's Documents with `command` as its arguments (as arguments, not
/// typed at its prompt, which loses letters outside ASCII); what it printed, or why it failed in
/// words.
fn afc(udid: &str, command: &[&str]) -> Result<String, String> {
    // after `--`, so the command's own options (ls -l) aren't taken for afcclient's
    let mut args = vec!["-u", udid, "--documents", PROCREATE, "--"];
    args.extend_from_slice(command);
    let out = run("afcclient", &args)?;
    if out.contains("ApplicationLookupFailed") || out.contains("InstallationLookupFailed") {
        return Err("Procreate isn't on this iPad (or doesn't share its folder).".into());
    }
    if out.to_lowercase().contains("lockdown") || out.to_lowercase().contains("pair") {
        return Err("The iPad isn't trusted yet: unlock it and tap Trust, then try again.".into());
    }
    if let Some(error) = out.lines().map(strip_ansi).find(|l| l.contains("Error:") || l.starts_with("ERROR")) {
        return Err(error.trim().to_owned());
    }
    Ok(out)
}

/// `program` run with `args`: what it printed (out and err together).
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program} couldn't be run: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}

/// Whether `program` is on the PATH.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let p = dir.join(program);
            p.is_file() && is_executable(&p)
        })
    })
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_p: &Path) -> bool {
    true
}

/// Whether usbmuxd answers on its socket (systemd may start it on the first connection).
#[cfg(unix)]
fn usbmuxd_running() -> bool {
    std::os::unix::net::UnixStream::connect(USBMUXD).is_ok()
}

#[cfg(not(unix))]
fn usbmuxd_running() -> bool {
    false
}

/// The previews of `files` (those that are artworks), read on a thread of its own through a
/// read-only `ifuse` mount at `at`, each sent as it comes (its name and PNG); the mount let go
/// when done. Without `ifuse`, none come.
fn read_previews(udid: String, files: Vec<IpadFile>, at: PathBuf) -> Receiver<(String, Vec<u8>)> {
    let (tx, rx) = channel();
    let unmount = if on_path("fusermount3") { "fusermount3" } else { "fusermount" };
    if !on_path("ifuse") || !on_path(unmount) {
        return rx;
    }
    let started = std::thread::Builder::new().name("ipad previews".into()).spawn(move || {
        let _ = std::fs::create_dir_all(&at);
        // a mount left from before goes first (lazily: it never waits on a device gone)
        let _ = run(unmount, &["-uz", &at.to_string_lossy()]);
        let mounted = Command::new("ifuse")
            .args(["-u", &udid, "--documents", PROCREATE, "-o", "ro"])
            .arg(&at)
            .output()
            .is_ok_and(|o| o.status.success());
        if !mounted {
            return;
        }
        for f in files.iter().filter(|f| f.name.to_lowercase().ends_with(".procreate")) {
            // the device's /Documents is the mount's top
            let local = at.join(f.path.trim_start_matches("/Documents/"));
            if let Some(png) = procraft_engine::zip::read_from_file(&local, PREVIEW)
                && tx.send((f.name.clone(), png)).is_err()
            {
                break;
            }
        }
        let _ = run(unmount, &["-uz", &at.to_string_lossy()]);
    });
    if let Err(e) = started {
        log::warn!("no iPad previews: {e}");
    }
    rx
}

/// What the browser is doing.
enum Stage {
    /// Looking for the iPad and what it holds.
    Looking(Receiver<Result<(String, Vec<IpadFile>), String>>),
    /// Its files shown, to choose from.
    Choosing,
    /// The ones chosen being copied over: how many came so far, of how many, and the copies.
    Copying(Receiver<Result<PathBuf, String>>, usize, usize, Vec<PathBuf>),
    /// Something stands in the way.
    Problem(String),
}

/// What the browser asks of the app.
pub enum Done {
    /// It closed with nothing brought in.
    Closed,
    /// These were copied over, to import.
    Import(Vec<PathBuf>),
}

/// The iPad's Procreate files, as a gallery of their previews, to choose from and bring in.
pub struct Browser {
    kind: Kind,
    stage: Stage,
    udid: String,
    files: Vec<IpadFile>,
    chosen: HashSet<String>,
    previews: HashMap<String, TextureHandle>,
    previews_coming: Option<Receiver<(String, Vec<u8>)>>,
    /// Where copies come to, and where the previews' mount is.
    copies: PathBuf,
    mount: PathBuf,
}

impl Browser {
    /// Opens it on `kind` of file, looking for the iPad and what it holds on a thread of its own.
    pub fn open(kind: Kind) -> Browser {
        let base = crate::files::app_dir();
        let copies = base.join("from-ipad");
        // what an earlier import copied over was taken in since
        let _ = std::fs::remove_dir_all(&copies);
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("ipad list".into()).spawn(move || {
            let _ = tx.send(ready().and_then(|udid| list(&udid, kind).map(|files| (udid, files))));
        });
        Browser {
            kind,
            stage: Stage::Looking(rx),
            udid: String::new(),
            files: Vec::new(),
            chosen: HashSet::new(),
            previews: HashMap::new(),
            previews_coming: None,
            copies,
            mount: base.join("ipad-mount"),
        }
    }

    /// Shows it over the gallery; what it asks, once it has its answer.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Done> {
        self.take_news(ctx);
        let screen = ctx.content_rect();
        let size = vec2((screen.width() - 80.0).min(900.0), (screen.height() - 80.0).min(640.0));
        let rect = Rect::from_center_size(screen.center(), size);
        let mut done = None;
        // the gallery dimmed behind it, a press there closing it
        egui::Area::new(egui::Id::unique("ipad veil")).fixed_pos(screen.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(0x99));
                if ui.interact(screen, ui.scope_id().with("ipad veil"), Sense::click()).clicked()
                    && !matches!(self.stage, Stage::Copying(..))
                {
                    done = Some(Done::Closed);
                }
            },
        );
        egui::Area::new(egui::Id::unique("ipad browser")).fixed_pos(rect.min).order(egui::Order::Tooltip).show(
            ctx,
            |ui| {
                ui.set_clip_rect(rect);
                ui.allocate_rect(rect, Sense::click());
                ui.painter().rect_filled(rect, 18, argb(pc::BAR));
                let ink = Color32::WHITE;
                text(
                    ui.painter(),
                    pos2(rect.left() + 24.0, rect.top() + 30.0),
                    Align2::LEFT_CENTER,
                    self.kind.title(),
                    20.0,
                    Weight::Bold,
                    ink,
                );
                let note = self.kind.note();
                text(
                    ui.painter(),
                    pos2(rect.left() + 24.0, rect.top() + 56.0),
                    Align2::LEFT_CENTER,
                    note,
                    13.0,
                    Weight::Regular,
                    argb(0xFF9A9A9A),
                );
                let body = Rect::from_min_max(
                    pos2(rect.left() + 16.0, rect.top() + 76.0),
                    pos2(rect.right() - 16.0, rect.bottom() - 70.0),
                );
                match &self.stage {
                    Stage::Looking(_) => {
                        text(
                            ui.painter(),
                            body.center(),
                            Align2::CENTER_CENTER,
                            "Looking at the iPad…",
                            16.0,
                            Weight::Regular,
                            ink,
                        );
                    }
                    Stage::Problem(why) => {
                        let galley = ui.painter().layout(
                            why.clone(),
                            egui::FontId::proportional(15.0),
                            ink,
                            body.width() - 40.0,
                        );
                        ui.painter().galley(body.center() - galley.size() / 2.0, galley, ink);
                    }
                    Stage::Choosing if self.files.is_empty() => {
                        let why = self.kind.empty();
                        let galley =
                            ui.painter().layout(why.into(), egui::FontId::proportional(15.0), ink, body.width() - 40.0);
                        ui.painter().galley(body.center() - galley.size() / 2.0, galley, ink);
                    }
                    Stage::Choosing => self.cards(ui, look, body),
                    Stage::Copying(_, got, of, _) => {
                        let said = format!("Copying {} of {}…", (got + 1).min(*of), of);
                        text(ui.painter(), body.center(), Align2::CENTER_CENTER, &said, 16.0, Weight::Regular, ink);
                    }
                }
                // Cancel, and Import with how many are chosen
                let bottom = rect.bottom() - 40.0;
                let cancel = Rect::from_center_size(pos2(rect.right() - 230.0, bottom), vec2(110.0, 40.0));
                let import = Rect::from_center_size(pos2(rect.right() - 90.0, bottom), vec2(140.0, 40.0));
                let copying = matches!(self.stage, Stage::Copying(..));
                if !copying {
                    ui.painter().rect_filled(cancel, 20, argb(0xFF3A3A3A));
                    text(ui.painter(), cancel.center(), Align2::CENTER_CENTER, "Cancel", 16.0, Weight::Regular, ink);
                    if ui.interact(cancel, ui.scope_id().with("ipad cancel"), Sense::click()).clicked() {
                        done = Some(Done::Closed);
                    }
                }
                let can_import = matches!(self.stage, Stage::Choosing) && !self.chosen.is_empty();
                let fill = if can_import { argb(pc::ACCENT) } else { argb(0xFF2E2E2E) };
                ui.painter().rect_filled(import, 20, fill);
                let label = if self.chosen.is_empty() {
                    "Import".to_owned()
                } else {
                    format!("Import ({})", self.chosen.len())
                };
                text(ui.painter(), import.center(), Align2::CENTER_CENTER, &label, 16.0, Weight::SemiBold, ink);
                if can_import && ui.interact(import, ui.scope_id().with("ipad import"), Sense::click()).clicked() {
                    self.start_copying();
                }
            },
        );
        if let Stage::Copying(_, got, of, copies) = &mut self.stage
            && *got == *of
        {
            done = Some(Done::Import(std::mem::take(copies)));
        }
        if matches!(self.stage, Stage::Looking(_) | Stage::Copying(..)) || self.previews_coming.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        done
    }

    /// The files as cards of their previews (newest Procreate first, as a gallery), a tap
    /// choosing or unchoosing one.
    fn cards(&mut self, ui: &mut Ui, _look: &mut Look, body: Rect) {
        let (card_w, card_h, gap) = (190.0, 190.0, 16.0);
        let across = ((body.width() + gap) / (card_w + gap)).floor().max(1.0) as usize;
        let rows = self.files.len().div_ceil(across);
        let builder = egui::UiBuilder::new().max_rect(body);
        ui.scope_builder(builder, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let (area, _) =
                    ui.allocate_exact_size(vec2(body.width() - 12.0, rows as f32 * (card_h + gap)), Sense::hover());
                for (i, f) in self.files.iter().enumerate() {
                    let (col, row) = (i % across, i / across);
                    let card = Rect::from_min_size(
                        area.min + vec2(col as f32 * (card_w + gap), row as f32 * (card_h + gap)),
                        vec2(card_w, card_h),
                    );
                    let picture = Rect::from_min_size(card.min, vec2(card_w, card_h - 44.0));
                    let chosen = self.chosen.contains(&f.name);
                    ui.painter().rect_filled(picture, 8, argb(0xFF2A2A2A));
                    match self.previews.get(&f.name) {
                        Some(t) => {
                            let [w, h] = t.size().map(|v| v as f32);
                            let k = (picture.width() / w).min(picture.height() / h);
                            let shown = Rect::from_center_size(picture.center(), vec2(w * k, h * k));
                            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                            ui.painter().image(t.id(), shown, uv, Color32::WHITE);
                        }
                        None => {
                            let kind = Path::new(&f.name)
                                .extension()
                                .map(|e| e.to_string_lossy().to_uppercase())
                                .unwrap_or_default();
                            text(
                                ui.painter(),
                                picture.center(),
                                Align2::CENTER_CENTER,
                                &kind,
                                14.0,
                                Weight::SemiBold,
                                argb(0xFF6A6A6A),
                            );
                        }
                    }
                    if chosen {
                        ui.painter().rect_stroke(
                            picture,
                            8,
                            egui::Stroke::new(3.0, argb(pc::ACCENT)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let stem =
                        Path::new(&f.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let name_at = pos2(card.left() + 2.0, picture.bottom() + 14.0);
                    let name = ui.painter().layout_no_wrap(stem, egui::FontId::proportional(14.0), Color32::WHITE);
                    ui.painter().with_clip_rect(card).galley(
                        name_at - vec2(0.0, name.size().y / 2.0),
                        name,
                        Color32::WHITE,
                    );
                    let size = format!("{:.1} MB", f.size as f64 / 1_000_000.0);
                    text(
                        ui.painter(),
                        pos2(card.left() + 2.0, picture.bottom() + 33.0),
                        Align2::LEFT_CENTER,
                        &size,
                        12.0,
                        Weight::Regular,
                        argb(0xFF9A9A9A),
                    );
                    if ui.interact(card, ui.scope_id().with(("ipad file", i)), Sense::click()).clicked() {
                        if chosen {
                            self.chosen.remove(&f.name);
                        } else {
                            self.chosen.insert(f.name.clone());
                        }
                    }
                }
            });
        });
    }

    /// What the threads have sent since the last frame.
    fn take_news(&mut self, ctx: &egui::Context) {
        if let Stage::Looking(rx) = &self.stage
            && let Ok(found) = rx.try_recv()
        {
            match found {
                Ok((udid, files)) => {
                    self.previews_coming = Some(read_previews(udid.clone(), files.clone(), self.mount.clone()));
                    self.udid = udid;
                    self.files = files;
                    self.stage = Stage::Choosing;
                }
                Err(why) => self.stage = Stage::Problem(why),
            }
        }
        if let Some(rx) = &self.previews_coming {
            loop {
                match rx.try_recv() {
                    Ok((name, png)) => {
                        if let Ok(img) = image::load_from_memory(&png) {
                            let rgba = img.to_rgba8();
                            let size = [rgba.width() as usize, rgba.height() as usize];
                            let picture = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                            let texture = ctx.load_texture(format!("ipad {name}"), picture, TextureOptions::LINEAR);
                            self.previews.insert(name, texture);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.previews_coming = None;
                        break;
                    }
                }
            }
        }
        // the iPad unplugged while its files show: said, not waited on
        if matches!(self.stage, Stage::Choosing) && !connected() && !self.udid.is_empty() {
            self.stage = Stage::Problem("The iPad was disconnected. Plug it in again and reopen Import.".into());
        }
        if let Stage::Copying(rx, got, _, copies) = &mut self.stage {
            while let Ok(result) = rx.try_recv() {
                match result {
                    Ok(path) => {
                        copies.push(path);
                        *got += 1;
                    }
                    Err(why) => {
                        self.stage = Stage::Problem(why);
                        break;
                    }
                }
            }
        }
    }

    /// The files chosen copied over, one after another, on a thread of their own.
    fn start_copying(&mut self) {
        let chosen: Vec<IpadFile> = self.files.iter().filter(|f| self.chosen.contains(&f.name)).cloned().collect();
        let (tx, rx) = channel();
        let (udid, dir, total) = (self.udid.clone(), self.copies.clone(), chosen.len());
        let _ = std::thread::Builder::new().name("ipad copy".into()).spawn(move || {
            for f in &chosen {
                let result = fetch(&udid, f, &dir);
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        self.stage = Stage::Copying(rx, 0, total, Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_gives_the_files_to_bring_in_names_with_spaces_and_all() {
        let out = "\u{1b}[0;30m\u{1b}[47mafc:\u{1b}[m\u{1b}[1;33m\u{1b}[44m/\u{1b}[m > drwxr-xr-x    2 mobile mobile         96 29 Jun 2026 19:48:45 Brushes\n\
                   -rw-r--r--    1 mobile mobile  236171583 03 Nov 2026 00:32:03 Xiǎoróu_Bag.procreate\n\
                   -rw-r--r--    1 mobile mobile     732600 20 Jul 2026 22:29:10 liliyth Outline.brushset\n\
                   -rw-r--r--    1 mobile mobile        100 20 Jul 2026 22:29:10 notes.txt\n";
        let files = parse_listing("/Documents", out, Kind::Artworks.extensions());
        assert_eq!(
            files,
            vec![IpadFile {
                path: "/Documents/Xiǎoróu_Bag.procreate".into(),
                name: "Xiǎoróu_Bag.procreate".into(),
                size: 236171583
            },]
        );
    }
}
