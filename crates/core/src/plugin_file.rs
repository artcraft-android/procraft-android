//! A plugin file: a zip with its own extension, holding `craft-extension.json` and exactly the
//! files it lists. All of it is hostile until read, so nothing is written until every entry has
//! been checked, and what is kept is read again in full on every start: a file that no longer
//! holds up counts as absent. A plugin says what it is; what it may do comes from how it got here,
//! which it has no way to claim.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use procraft_engine::zip::ZipReader;
use serde::Deserialize;
use serde_json::Value;

use crate::patch_run::{self, Hook, MODULE, MOST_MODULE};
use crate::plugins::{self, PluginControl, PluginKind, PluginSlider, PluginSwitch};

/// The manifest's name in a plugin file, and in its folder.
const MANIFEST: &str = "craft-extension.json";
/// The manifest format read here: a newer one may mean anything, so it is refused.
const SCHEMA: u64 = 1;
/// What a plugin's code is written to, see [`crate::patch_run`]: a newer one is refused too.
const INTERFACE: u32 = 1;
/// What ids of our own start with: no file may claim one.
const OWN_PREFIX: &str = "procraft.";
/// The file, its entries, one entry, all of them unpacked, and the manifest, in bytes.
pub(crate) const MOST_FILE: usize = 32 << 20;
const MOST_ENTRIES: usize = 64;
const MOST_ENTRY: usize = 16 << 20;
const MOST_UNPACKED: usize = 64 << 20;
const MOST_MANIFEST: usize = 64 << 10;
const MOST_SHADER: usize = 64 << 10;
/// A filter's code in its folder.
pub(crate) const FILTER_CODE: &str = "filter.wgsl";
/// How far an entry may shrink in the archive, from this size on: text never gets near it.
const MOST_RATIO: usize = 100;
const RATIO_FROM: usize = 64 << 10;
/// How deep a listed path goes, how long it is, and how many controls and hooks a plugin has.
const MOST_DEPTH: usize = 8;
const MOST_PATH: usize = 200;
const MOST_CONTROLS: usize = 8;
const MOST_HOOKS: usize = 8;
/// The reason a stopped patch gives, in bytes.
const MOST_STOP_NOTE: usize = 200;
/// A slider's stops: past this a range and step were not meant.
const MOST_STOPS: f64 = 100_000.0;

/// Why a file wasn't taken. Each says so in plain words, once; the detail goes to the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotAPlugin,
    TooBig,
    Unsafe,
    Mismatch,
    Damaged,
    Described,
    Permissions,
    Code,
    FilterCode,
    Newer,
    OtherApp,
    NewerInstalled,
    Clash,
    NotKept,
}

impl Refusal {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Refusal::NotAPlugin => "This isn't a ProCraft plugin.",
            Refusal::TooBig => "This plugin is too big to import.",
            Refusal::Unsafe => "This plugin file isn't safe to open.",
            Refusal::Mismatch => "This plugin's files don't match its list.",
            Refusal::Damaged => "This plugin file is damaged.",
            Refusal::Described => "This plugin describes itself wrongly.",
            Refusal::Permissions => "This plugin asks for permissions ProCraft doesn't give.",
            Refusal::Code => "This patch's code can't run in ProCraft.",
            Refusal::FilterCode => "This filter's code can't run in ProCraft.",
            Refusal::Newer => "This plugin needs a newer ProCraft.",
            Refusal::OtherApp => "This plugin isn't for ProCraft.",
            Refusal::NewerInstalled => "A newer version of this plugin is already imported.",
            Refusal::Clash => "Another plugin already holds a setting this one needs.",
            Refusal::NotKept => "This plugin couldn't be saved.",
        }
    }
}

/// A file a manifest lists.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    path: String,
    sha256: String,
    size: usize,
}

/// A manifest that has held up, its controls' keys carrying the plugin's id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    pub id: String,
    pub name: String,
    pub kind: PluginKind,
    pub version: String,
    pub publisher: String,
    pub description: String,
    pub experimental: bool,
    /// Where a patch steps in; none for a filter.
    pub hooks: Vec<Hook>,
    pub controls: Vec<PluginControl>,
    files: Vec<Listed>,
}

/// A plugin file read and checked, ready to keep.
pub(crate) struct Package {
    pub manifest: Manifest,
    text: Vec<u8>,
    files: Vec<(String, Vec<u8>)>,
}

#[derive(Deserialize)]
struct Head {
    schema: u64,
}

#[derive(Deserialize)]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    kind: String,
    #[serde(default)]
    publisher: Option<RawPublisher>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    experimental: Option<bool>,
    #[serde(default)]
    compat: RawCompat,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    permissions: Vec<Value>,
    #[serde(default)]
    hooks: Vec<String>,
    #[serde(default)]
    controls: Vec<RawControl>,
}

#[derive(Deserialize)]
struct RawPublisher {
    name: String,
}

#[derive(Default, Deserialize)]
struct RawCompat {
    procraft: Option<RawAppCompat>,
}

#[derive(Deserialize)]
struct RawAppCompat {
    #[serde(default)]
    min_app: Option<String>,
    #[serde(default)]
    interface: Option<u32>,
}

#[derive(Deserialize)]
struct RawFile {
    path: String,
    sha256: String,
    size: usize,
}

#[derive(Deserialize)]
struct RawControl {
    key: String,
    #[serde(rename = "type")]
    kind: String,
    label: String,
    #[serde(default)]
    least: Option<f64>,
    #[serde(default)]
    most: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    starts: Option<Value>,
    #[serde(default)]
    live: Option<bool>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    at_least: Option<String>,
    #[serde(default)]
    shown_times: Option<f64>,
    /// A setting the app used to have, whose kept value this control takes over.
    #[serde(default)]
    keeps: Option<String>,
}

/// Whether `text`, at most `most` characters, is fit to show: nothing that hides, reorders or
/// imitates what is around it (controls, bidi marks, zero-width and other invisible characters).
fn shows_plainly(text: &str, most: usize) -> bool {
    text.chars().count() <= most && !text.chars().any(|c| c.is_control() || invisible(c))
}

/// Characters drawn as nothing, or that move the text around them.
fn invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0xAD | 0x34F | 0x61C | 0x115F..=0x1160 | 0x17B4..=0x17B5 | 0x180B..=0x180E
            | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0x3164
            | 0xE000..=0xF8FF | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFFB
            | 0xE0000..=0xE0FFF | 0xF0000..=0x10FFFF
    )
}

/// A name Windows keeps for itself, with or without an extension.
fn windows_keeps(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    let numbered =
        |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.bytes().all(|b| b.is_ascii_digit()));
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul") || numbered("com") || numbered("lpt")
}

/// An id: lowercase, dotted, each part plain, and at least two of them. A bundled plugin's id has
/// no dot, so no key of an imported plugin can be one of theirs.
fn valid_id(id: &str) -> bool {
    let plain = |part: &str| {
        !part.is_empty()
            && !part.starts_with('-')
            && !part.ends_with('-')
            && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    id.len() <= 100 && id.split('.').count() >= 2 && id.split('.').all(plain) && !windows_keeps(id)
}

/// A control's key: it goes in a settings line, after the plugin's id, so it is one plain word.
fn valid_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    key.len() <= 32
        && key != "enabled"
        && key != "applied"
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `MAJOR.MINOR.PATCH` in numbers, also a folder's name.
fn valid_version(version: &str) -> bool {
    version_parts(version).is_some() && !version.contains('-') && version.len() <= 24
}

/// A version's numbers, what comes after a `-` left out.
fn version_parts(version: &str) -> Option<[u32; 3]> {
    let numbers = version.split('-').next()?;
    let mut parts = numbers
        .split('.')
        .map(|p| (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse::<u32>().ok()).flatten());
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(version)
}

/// Whether `version` comes before `other` (neither being a version says nothing either way).
pub(crate) fn version_is_older(version: &str, other: &str) -> bool {
    version_parts(version).zip(version_parts(other)).is_some_and(|(a, b)| a < b)
}

/// A path inside a plugin: relative, plain at every step, never out of its folder.
fn safe_path(path: &str) -> bool {
    let plain = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && !part.ends_with('.')
            && !windows_keeps(part)
            && part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    path.len() <= MOST_PATH && path.split('/').count() <= MOST_DEPTH && path.split('/').all(plain)
}

fn sha_digits(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `control` as the list has it, its key under the plugin's id.
fn checked_control(id: &str, raw: RawControl) -> Result<PluginControl, Refusal> {
    if !valid_key(&raw.key) || raw.label.trim().is_empty() || !shows_plainly(&raw.label, 40) {
        return Err(Refusal::Described);
    }
    // what the app no longer has stays where it was kept, for the patch that took it over
    let key = match &raw.keeps {
        Some(kept) if plugins::moved(kept) => kept.clone(),
        Some(_) => return Err(Refusal::Described),
        None => format!("{id}.{}", raw.key),
    };
    match raw.kind.as_str() {
        "switch" => {
            let starts = match raw.starts {
                None => false,
                Some(Value::Bool(on)) => on,
                Some(_) => return Err(Refusal::Described),
            };
            let control = PluginSwitch { key, label: raw.label, on: starts, starts };
            Ok(PluginControl::Switch { control })
        }
        "slider" => {
            let (least, most, step) =
                (raw.least.unwrap_or(f64::NAN), raw.most.unwrap_or(f64::NAN), raw.step.unwrap_or(1.0));
            let shown_times = raw.shown_times.unwrap_or(1.0);
            let finite = [least, most, step, shown_times].iter().all(|v| v.is_finite() && v.abs() < 1e9);
            if !finite || least >= most || step <= 0.0 || shown_times <= 0.0 || (most - least) / step > MOST_STOPS {
                return Err(Refusal::Described);
            }
            let (unit, at_least) = (raw.unit.unwrap_or_default(), raw.at_least);
            if !shows_plainly(&unit, 8) || at_least.as_deref().is_some_and(|text| !shows_plainly(text, 16)) {
                return Err(Refusal::Described);
            }
            let starts = match raw.starts {
                None => least,
                Some(value) => value.as_f64().filter(|v| v.is_finite()).ok_or(Refusal::Described)?,
            };
            let mut control = PluginSlider {
                key,
                label: raw.label,
                least: least as f32,
                most: most as f32,
                step: step as f32,
                value: 0.0,
                starts: 0.0,
                shown_times: shown_times as f32,
                unit,
                at_least,
                live: raw.live.unwrap_or(false),
            };
            // kept to the range and on a step, however it was written
            control.starts = control.stepped(starts as f32);
            control.value = control.starts;
            Ok(PluginControl::Slider { control })
        }
        // a kind of control this app doesn't have
        _ => Err(Refusal::Newer),
    }
}

/// `text` as a manifest, if it holds up.
fn read_manifest(text: &[u8]) -> Result<Manifest, Refusal> {
    if text.len() > MOST_MANIFEST {
        return Err(Refusal::TooBig);
    }
    // the schema first: a newer format is refused as newer, whatever else it holds
    let head: Head = serde_json::from_slice(text).map_err(|_| Refusal::NotAPlugin)?;
    if head.schema > SCHEMA {
        return Err(Refusal::Newer);
    }
    if head.schema < SCHEMA {
        return Err(Refusal::NotAPlugin);
    }
    // (a field given twice is an error here, so a manifest can't say two things to two readers)
    let raw: RawManifest = serde_json::from_slice(text).map_err(|_| Refusal::Described)?;
    if !raw.permissions.is_empty() {
        return Err(Refusal::Permissions);
    }
    let compat = raw.compat.procraft.ok_or(Refusal::OtherApp)?;
    let kind = match raw.kind.as_str() {
        "filter" => PluginKind::Filter,
        "patch" => PluginKind::Patch,
        _ => return Err(Refusal::Newer),
    };
    if compat.interface.is_some_and(|interface| interface > INTERFACE) {
        return Err(Refusal::Newer);
    }
    let publisher = raw.publisher.map(|p| p.name).unwrap_or_default();
    let named = !raw.name.trim().is_empty() && shows_plainly(&raw.name, 60);
    if !valid_id(&raw.id) || !valid_version(&raw.version) || !named {
        return Err(Refusal::Described);
    }
    if !shows_plainly(&raw.description, 200) || !shows_plainly(&publisher, 60) {
        return Err(Refusal::Described);
    }
    if compat.min_app.as_deref().is_some_and(|v| version_parts(v).is_none()) || compat.interface == Some(0) {
        return Err(Refusal::Described);
    }
    let mut files = Vec::with_capacity(raw.files.len().min(MOST_ENTRIES));
    let mut seen = HashSet::new();
    for file in raw.files {
        let fits = safe_path(&file.path) && file.path != MANIFEST && sha_digits(&file.sha256);
        if !fits || !seen.insert(file.path.to_ascii_lowercase()) {
            return Err(Refusal::Described);
        }
        files.push(Listed { path: file.path, sha256: file.sha256, size: file.size });
    }
    if raw.controls.len() > MOST_CONTROLS {
        return Err(Refusal::Described);
    }
    let mut keys = HashSet::new();
    if !raw.controls.iter().all(|c| keys.insert(c.key.clone())) {
        return Err(Refusal::Described);
    }
    let id = raw.id;
    let controls = raw.controls.into_iter().map(|c| checked_control(&id, c)).collect::<Result<Vec<_>, _>>()?;
    if id.starts_with(OWN_PREFIX) {
        return Err(Refusal::Described);
    }
    let mut hooks = Vec::new();
    if kind == PluginKind::Patch {
        // a hook this app doesn't have is a patch for a newer one
        for name in raw.hooks.iter().take(MOST_HOOKS + 1) {
            let hook = Hook::named(name).ok_or(Refusal::Newer)?;
            if hooks.contains(&hook) {
                return Err(Refusal::Described);
            }
            hooks.push(hook);
        }
        let module = files.iter().any(|f| f.path == MODULE && f.size <= MOST_MODULE);
        if !module || hooks.is_empty() || compat.interface.is_none() {
            return Err(Refusal::Described);
        }
    } else {
        let code = files.iter().any(|f| f.path == FILTER_CODE && f.size <= MOST_SHADER);
        let moved = controls.iter().any(|c| !c.key().starts_with(&format!("{id}.")));
        if !code || moved || compat.interface.is_none() {
            return Err(Refusal::Described);
        }
    }
    Ok(Manifest {
        id,
        name: raw.name,
        kind,
        version: raw.version,
        publisher,
        description: raw.description,
        experimental: raw.experimental.unwrap_or(true),
        hooks,
        controls,
        files,
    })
}

/// What a filter's code calls `controls`, in their order: their keys, without the plugin's id.
pub(crate) fn value_names(controls: &[PluginControl]) -> Vec<String> {
    controls.iter().map(|control| control.key().rsplit('.').next().unwrap_or("").to_owned()).collect()
}

/// Whether `data`, listed by `manifest` under `path`, is code that can run, if it is code at all:
/// checked by reading it, as nothing in it runs until the patch is applied or the filter opened.
fn code_holds(manifest: &Manifest, path: &str, data: &[u8]) -> bool {
    if manifest.kind == PluginKind::Filter && path == FILTER_CODE {
        let text = std::str::from_utf8(data).map_err(|e| e.to_string());
        let read = text.and_then(|text| procraft_gpu::read_filter(text, &value_names(&manifest.controls)));
        return read.map_err(|why| log::warn!("a filter's code was refused: {why}")).is_ok();
    }
    let controls = !manifest.controls.is_empty();
    manifest.kind != PluginKind::Patch || path != MODULE || patch_run::can_run(data, &manifest.hooks, controls)
}

/// `bytes` read as a plugin file. Every entry is checked by what it claims before any is read,
/// and what is read can't pass what it claimed, so a bomb costs nothing and nothing is written.
pub(crate) fn read_package(bytes: &[u8]) -> Result<Package, Refusal> {
    if bytes.len() > MOST_FILE {
        return Err(Refusal::TooBig);
    }
    let zip = ZipReader::new(bytes).ok_or(Refusal::NotAPlugin)?;
    let mut seen = HashSet::new();
    let (mut count, mut unpacked) = (0_usize, 0_usize);
    for entry in zip.facts() {
        count += 1;
        unpacked = unpacked.saturating_add(entry.size);
        let manifest_too_big = entry.name == MANIFEST && entry.size > MOST_MANIFEST;
        if count > MOST_ENTRIES || entry.size > MOST_ENTRY || unpacked > MOST_UNPACKED || manifest_too_big {
            return Err(Refusal::TooBig);
        }
        let shrunk = entry.size >= RATIO_FROM && entry.size / entry.compressed.max(1) > MOST_RATIO;
        let path = entry.name == MANIFEST || safe_path(entry.name);
        if !entry.plain || entry.encrypted || shrunk || !path || !seen.insert(entry.name.to_ascii_lowercase()) {
            return Err(Refusal::Unsafe);
        }
    }
    if !zip.names_agree() {
        return Err(Refusal::Unsafe);
    }
    zip.size(MANIFEST).ok_or(Refusal::NotAPlugin)?;
    let text = zip.read(MANIFEST).ok_or(Refusal::Damaged)?;
    let manifest = read_manifest(&text)?;
    // exactly what is listed: nothing more, nothing less
    if seen.len() != manifest.files.len() + 1 {
        return Err(Refusal::Mismatch);
    }
    let mut files = Vec::with_capacity(manifest.files.len());
    for listed in &manifest.files {
        if zip.size(&listed.path) != Some(listed.size) {
            return Err(Refusal::Mismatch);
        }
        let data = zip.read(&listed.path).ok_or(Refusal::Damaged)?;
        if sha256(&data) != listed.sha256 {
            return Err(Refusal::Mismatch);
        }
        if !code_holds(&manifest, &listed.path, &data) {
            return Err(if manifest.kind == PluginKind::Patch { Refusal::Code } else { Refusal::FilterCode });
        }
        files.push((listed.path.clone(), data));
    }
    Ok(Package { manifest, text, files })
}

/// What a plugin file at `path` holds, read if it is a plain file of a size worth reading: a pipe
/// would block for ever, a device has no end.
pub fn read_plugin_file(path: &Path) -> Option<Vec<u8>> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MOST_FILE as u64 + 1).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Writes `data` as a new file, on the disk when this returns.
fn write_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

/// A name in `dir` nothing else has, for work under way: it starts with a dot, so a start finding
/// one knows it was left behind.
fn spare(dir: &Path, what: &str) -> PathBuf {
    static MADE: AtomicU64 = AtomicU64::new(0);
    dir.join(format!(".{what}-{}-{}", std::process::id(), MADE.fetch_add(1, Ordering::Relaxed)))
}

/// Keeps `package` in `dir` as `<id>/<version>/`, whole or not at all: written apart, then moved
/// into place, and any other version of it goes. Returns its folder.
pub(crate) fn keep(dir: &Path, package: &Package) -> Result<PathBuf, Refusal> {
    let stage = spare(dir, "staging");
    let staged = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&stage)?;
        write_new(&stage.join(MANIFEST), &package.text)?;
        for (path, data) in &package.files {
            let to = stage.join(path);
            if let Some(folder) = to.parent() {
                std::fs::create_dir_all(folder)?;
            }
            write_new(&to, data)?;
        }
        Ok(())
    })();
    if let Err(e) = staged {
        log::warn!("a plugin's files couldn't be written: {e}");
        let _ = std::fs::remove_dir_all(&stage);
        return Err(Refusal::NotKept);
    }
    let home = dir.join(&package.manifest.id);
    let folder = home.join(&package.manifest.version);
    let moved = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&home)?;
        // the old one kept aside until the new one is in place
        let old = spare(dir, "replaced");
        let had = std::fs::rename(&folder, &old).is_ok();
        if let Err(e) = std::fs::rename(&stage, &folder) {
            if had {
                let _ = std::fs::rename(&old, &folder);
            }
            return Err(e);
        }
        if had {
            let _ = std::fs::remove_dir_all(&old);
        }
        Ok(())
    })();
    if let Err(e) = moved {
        log::warn!("a plugin couldn't be moved into place: {e}");
        let _ = std::fs::remove_dir_all(&stage);
        return Err(Refusal::NotKept);
    }
    // other versions go, now this one stands
    if let Ok(versions) = std::fs::read_dir(&home) {
        for version in versions.flatten() {
            if version.file_name().to_str() != Some(package.manifest.version.as_str()) {
                let _ = std::fs::remove_dir_all(version.path());
            }
        }
    }
    Ok(folder)
}

/// Takes plugin `id`'s folder out of `dir`: gone from the next read on, even if what is moved
/// aside can't be deleted now (the next start tries again). False if it stays.
pub(crate) fn forget(dir: &Path, id: &str) -> bool {
    clear_stopped(dir, id);
    let home = dir.join(id);
    let aside = spare(dir, "removed");
    if std::fs::rename(&home, &aside).is_err() {
        return !home.exists();
    }
    let _ = std::fs::remove_dir_all(&aside);
    true
}

/// The paths of the plain files under `folder`, `prefix` before each; none if it holds anything
/// else, or more than a plugin may.
fn files_under(folder: &Path, prefix: &str, depth: usize, found: &mut Vec<String>) -> Option<()> {
    if depth > MOST_DEPTH {
        return None;
    }
    for entry in std::fs::read_dir(folder).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        let path = format!("{prefix}{name}");
        // (a link says it is one here, whatever it points at)
        let kind = entry.file_type().ok()?;
        if kind.is_dir() {
            files_under(&entry.path(), &format!("{path}/"), depth + 1, found)?;
        } else if kind.is_file() {
            found.push(path);
        } else {
            return None;
        }
        if found.len() > MOST_ENTRIES {
            return None;
        }
    }
    Some(())
}

/// The plugin in `folder`, if all of it is as its manifest says and it is `id` at `version`.
pub(crate) fn read_folder(folder: &Path, id: &str, version: &str) -> Option<Manifest> {
    let mut found = Vec::new();
    files_under(folder, "", 0, &mut found)?;
    let text = read_capped(&folder.join(MANIFEST), MOST_MANIFEST)?;
    let manifest = read_manifest(&text).ok()?;
    if manifest.id != id || manifest.version != version || found.len() != manifest.files.len() + 1 {
        return None;
    }
    for listed in &manifest.files {
        if !found.contains(&listed.path) {
            return None;
        }
        let data = read_capped(&folder.join(&listed.path), listed.size)?;
        if data.len() != listed.size || sha256(&data) != listed.sha256 {
            return None;
        }
        if !code_holds(&manifest, &listed.path, &data) {
            return None;
        }
    }
    Some(manifest)
}

/// The hooks and the module of the patch `id` at `version` in `dir`, if all of it holds up as it
/// did.
pub(crate) fn read_code(dir: &Path, id: &str, version: &str) -> Option<(Vec<Hook>, Vec<u8>)> {
    let folder = dir.join(id).join(version);
    let manifest = read_folder(&folder, id, version)?;
    Some((manifest.hooks, read_capped(&folder.join(MODULE), MOST_MODULE)?))
}

/// The manifest and the code of the filter `id` at `version` in `dir`, if all of it holds up as it
/// did.
pub(crate) fn read_filter_code(dir: &Path, id: &str, version: &str) -> Option<(Manifest, String)> {
    let folder = dir.join(id).join(version);
    let manifest = read_folder(&folder, id, version)?;
    let text = String::from_utf8(read_capped(&folder.join(FILTER_CODE), MOST_SHADER)?).ok()?;
    Some((manifest, text))
}

/// Where it is written down which filter is on trial: compiling, or not yet drawn. It is there on
/// a start only if the app ended meanwhile.
fn trying_note(dir: &Path) -> PathBuf {
    dir.join("filter-trying")
}

/// Writes down that filter `id` is on trial, on the disk before anything happens to it.
pub(crate) fn note_trying(dir: &Path, id: &str) {
    if let Err(e) = write_replacing(&trying_note(dir), id.as_bytes()) {
        log::warn!("couldn't note a filter on trial: {e}");
    }
}

pub(crate) fn clear_trying(dir: &Path) {
    let _ = std::fs::remove_file(trying_note(dir));
}

/// `data` as `path`, on the disk when this returns.
fn write_replacing(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}

/// A filter on trial when the app last ended is written down as stopped, `why`.
pub(crate) fn settle_trying(dir: &Path, why: &str) {
    if let Some(id) = read_capped(&trying_note(dir), MOST_STOP_NOTE).and_then(|id| String::from_utf8(id).ok()) {
        log::warn!("the app ended with a filter on trial: {id}");
        note_stopped(dir, &id, why);
    }
    clear_trying(dir);
}

/// Where it is written down that patch `id` stopped: beside its folder, which holds only what its
/// manifest lists.
fn stopped_note(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.stopped"))
}

/// Writes down why patch `id` stopped, so it stays off through a restart.
pub(crate) fn note_stopped(dir: &Path, id: &str, why: &str) {
    if let Err(e) = std::fs::write(stopped_note(dir, id), why) {
        log::warn!("couldn't note a patch stopped: {e}");
    }
}

/// Why patch `id` stopped, if it did and nobody has asked it to run since.
pub(crate) fn stopped_why(dir: &Path, id: &str) -> Option<String> {
    String::from_utf8(read_capped(&stopped_note(dir, id), MOST_STOP_NOTE)?).ok()
}

pub(crate) fn clear_stopped(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(stopped_note(dir, id));
}

/// Where it is written down which moved settings someone has been told of, a line each.
fn told_note(dir: &Path) -> PathBuf {
    dir.join("told")
}

/// The moved settings someone has been told of.
pub(crate) fn read_told(dir: &Path) -> Vec<String> {
    let text = read_capped(&told_note(dir), MOST_STOP_NOTE).unwrap_or_default();
    String::from_utf8_lossy(&text).lines().map(str::to_owned).collect()
}

/// Writes down that someone has been told of `told`, so it is said once.
pub(crate) fn note_told(dir: &Path, told: &[String]) {
    let written = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(told_note(dir), told.join("\n")));
    if let Err(e) = written {
        log::warn!("couldn't note what has been told: {e}");
    }
}

/// `path`, if it is a plain file of at most `most` bytes.
fn read_capped(path: &Path, most: usize) -> Option<Vec<u8>> {
    let facts = std::fs::symlink_metadata(path).ok()?;
    if !facts.is_file() || facts.len() > most as u64 {
        return None;
    }
    std::fs::read(path).ok()
}

/// Every plugin kept in `dir` that holds up, the newest of each. What was left half done goes; a
/// folder that doesn't hold up is passed over and left, in case the fault is ours.
pub(crate) fn read_all(dir: &Path) -> Vec<Manifest> {
    let mut kept = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return kept };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str() else { continue };
        if id.starts_with('.') {
            let _ = std::fs::remove_dir_all(entry.path());
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || !valid_id(id) {
            continue;
        }
        let Ok(versions) = std::fs::read_dir(entry.path()) else { continue };
        let mut newest: Option<Manifest> = None;
        for version in versions.flatten() {
            let folder = version.path();
            let found = version.file_name().to_str().and_then(|v| {
                let plain = version.file_type().is_ok_and(|kind| kind.is_dir());
                plain.then(|| read_folder(&folder, id, v)).flatten()
            });
            match found {
                Some(manifest) if newest.as_ref().is_none_or(|n| version_is_older(&n.version, &manifest.version)) => {
                    newest = Some(manifest);
                }
                Some(_) => {}
                None => log::warn!("a kept plugin doesn't hold up: {}", folder.display()),
            }
        }
        kept.extend(newest);
    }
    kept.sort_by(|a, b| a.id.cmp(&b.id));
    kept
}

/// SHA-256 of `data`, in lowercase hex.
pub(crate) fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    message.resize(message.len() + (64 + 56 - message.len() % 64) % 64, 0);
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..64 {
            let at = |back: usize| w.get(i - back).copied().unwrap_or(0);
            let (a, b) = (at(15), at(2));
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            let word = at(16).wrapping_add(s0).wrapping_add(at(7)).wrapping_add(s1);
            if let Some(slot) = w.get_mut(i) {
                *slot = word;
            }
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut last] = h;
        for (k, word) in K.iter().zip(&w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let t1 = last.wrapping_add(s1).wrapping_add(choice).wrapping_add(*k).wrapping_add(*word);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            last = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (sum, add) in h.iter_mut().zip([a, b, c, d, e, f, g, last]) {
            *sum = sum.wrapping_add(add);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use procraft_engine::zip::ZipWriter;
    use serde_json::json;

    use super::*;

    pub(crate) const SHADER: &[u8] = b"fn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return colour; }";

    /// A manifest as a test starts from: a filter, valid, listing [`SHADER`].
    pub(crate) fn filter(id: &str) -> Value {
        json!({
            "schema": 1,
            "id": id,
            "name": "A filter",
            "version": "1.0.0",
            "kind": "filter",
            "publisher": { "name": "Someone" },
            "description": "Changes a layer.",
            "compat": { "procraft": { "min_app": "0.0.1", "interface": 1 } },
            "files": [{ "path": "filter.wgsl", "sha256": sha256(SHADER), "size": SHADER.len() }],
            "permissions": [],
            "controls": [
                { "key": "cut", "type": "slider", "label": "Cut-off", "least": 0, "most": 1, "step": 0.01, "starts": 0.5 },
                { "key": "keep_colour", "type": "switch", "label": "Keep the colour", "starts": false },
            ],
        })
    }

    /// A manifest for a patch of `module`, valid, on the position hook, with no controls.
    pub(crate) fn patch(id: &str, module: &[u8]) -> Value {
        json!({
            "schema": 1,
            "id": id,
            "name": "A patch",
            "version": "1.0.0",
            "kind": "patch",
            "publisher": { "name": "Someone" },
            "description": "Changes the app.",
            "compat": { "procraft": { "min_app": "0.0.1", "interface": 1 } },
            "files": [{ "path": MODULE, "sha256": sha256(module), "size": module.len() }],
            "permissions": [],
            "hooks": ["position"],
        })
    }

    /// A plugin file of `manifest` and `files`, as they are.
    pub(crate) fn pack(manifest: &Value, files: &[(&str, &[u8])]) -> Vec<u8> {
        pack_text(manifest.to_string().as_bytes(), files)
    }

    fn pack_text(manifest: &[u8], files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new();
        zip.add(MANIFEST, manifest);
        for (name, data) in files {
            zip.add(name, data);
        }
        zip.finish()
    }

    fn refusal(bytes: &[u8]) -> Refusal {
        read_package(bytes).err().expect("it was taken")
    }

    fn with(mut manifest: Value, path: &str, value: Value) -> Value {
        *manifest.pointer_mut(path).expect(path) = value;
        manifest
    }

    fn a_filter() -> Value {
        filter("org.example.filter")
    }

    fn a_filter_file(manifest: &Value) -> Vec<u8> {
        pack(manifest, &[("filter.wgsl", SHADER)])
    }

    #[test]
    fn sha_256_matches_the_published_vectors() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let two_blocks = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(sha256(two_blocks), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        // a million a's: the padding falling at every place in a block on the way
        let million = vec![b'a'; 1_000_000];
        assert_eq!(sha256(&million), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn a_good_filter_is_read() {
        let package = read_package(&a_filter_file(&a_filter())).unwrap();
        assert_eq!(package.manifest.id, "org.example.filter");
        assert_eq!(package.files.len(), 1);
    }

    #[test]
    fn what_isn_t_a_plugin_is_said_to_be() {
        assert_eq!(refusal(b"not a zip at all"), Refusal::NotAPlugin);
        let mut zip = ZipWriter::new();
        zip.add("something.txt", b"hello");
        assert_eq!(refusal(&zip.finish()), Refusal::NotAPlugin);
        assert_eq!(refusal(&pack_text(b"{ not json", &[])), Refusal::NotAPlugin);
    }

    #[test]
    fn a_path_out_of_the_folder_is_refused() {
        // in the archive, unlisted, and listed: each way round
        for path in
            ["../evil.txt", "a/../../evil.txt", "/etc/evil", "c:/evil", "a\\..\\evil", "a//b", "a/./b", ".hidden"]
        {
            assert_eq!(
                refusal(&pack(&a_filter(), &[("filter.wgsl", SHADER), (path, b"x")])),
                Refusal::Unsafe,
                "{path}"
            );
            let listed = with(a_filter(), "/files/0/path", json!(path));
            assert_eq!(refusal(&pack(&listed, &[(path, SHADER)])), Refusal::Unsafe, "{path}");
        }
        for name in ["con", "NUL.txt", "lpt1", "com9.wgsl", "aux.wgsl"] {
            assert!(!safe_path(name), "{name}");
        }
    }

    /// The first central entry of `bytes` with these attributes.
    fn with_attributes(mut bytes: Vec<u8>, name: &str, attributes: u32) -> Vec<u8> {
        let mut at = 0;
        loop {
            let found = bytes[at..].windows(4).position(|w| w == [0x50, 0x4B, 0x01, 0x02]).expect("no such entry") + at;
            let len = usize::from(u16::from_le_bytes([bytes[found + 28], bytes[found + 29]]));
            if &bytes[found + 46..found + 46 + len] == name.as_bytes() {
                bytes[found + 38..found + 42].copy_from_slice(&attributes.to_le_bytes());
                return bytes;
            }
            at = found + 4;
        }
    }

    #[test]
    fn a_link_or_a_device_is_refused() {
        let file = a_filter_file(&a_filter());
        let link = with_attributes(file.clone(), "filter.wgsl", 0o120_777 << 16);
        assert_eq!(refusal(&link), Refusal::Unsafe);
        let fifo = with_attributes(file.clone(), "filter.wgsl", 0o010_644 << 16);
        assert_eq!(refusal(&fifo), Refusal::Unsafe);
        let folder = with_attributes(file, "filter.wgsl", 0x10);
        assert_eq!(refusal(&folder), Refusal::Unsafe);
    }

    #[test]
    fn a_bomb_is_refused_by_what_it_claims() {
        // two megabytes of nothing, a few kilobytes in the archive
        let zeros = vec![0_u8; 2 << 20];
        let manifest =
            with(with(a_filter(), "/files/0/sha256", json!(sha256(&zeros))), "/files/0/size", json!(zeros.len()));
        assert_eq!(refusal(&pack(&manifest, &[("filter.wgsl", &zeros)])), Refusal::Unsafe);
    }

    #[test]
    fn too_much_is_refused() {
        // a file over the cap, whatever is in it
        assert_eq!(refusal(&vec![0_u8; MOST_FILE + 1]), Refusal::TooBig);
        // too many entries
        let many: Vec<(String, &[u8])> = (0..=MOST_ENTRIES).map(|i| (format!("f{i}.txt"), &b"x"[..])).collect();
        let entries: Vec<(&str, &[u8])> = many.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        assert_eq!(refusal(&pack(&a_filter(), &entries)), Refusal::TooBig);
        // a manifest past its cap, padded with a field nobody reads
        let padded = with(a_filter(), "/description", json!("x"));
        let mut padded = padded.as_object().unwrap().clone();
        padded.insert("padding".to_owned(), json!("x".repeat(MOST_MANIFEST + 1)));
        assert_eq!(refusal(&pack(&Value::Object(padded), &[("filter.wgsl", SHADER)])), Refusal::TooBig);
    }

    #[test]
    fn a_file_that_isn_t_as_listed_is_refused() {
        // a hash that doesn't match
        let wrong = with(a_filter(), "/files/0/sha256", json!(sha256(b"something else")));
        assert_eq!(refusal(&a_filter_file(&wrong)), Refusal::Mismatch);
        // a size that doesn't
        let wrong = with(a_filter(), "/files/0/size", json!(SHADER.len() + 1));
        assert_eq!(refusal(&a_filter_file(&wrong)), Refusal::Mismatch);
        // a file more than the list has, and one less
        let extra = pack(&a_filter(), &[("filter.wgsl", SHADER), ("extra.txt", b"x")]);
        assert_eq!(refusal(&extra), Refusal::Mismatch);
        assert_eq!(refusal(&pack(&a_filter(), &[])), Refusal::Mismatch);
        // the same name twice, and by case
        let twice = pack(&a_filter(), &[("filter.wgsl", SHADER), ("filter.wgsl", SHADER)]);
        assert_eq!(refusal(&twice), Refusal::Unsafe);
        let cased = pack(&a_filter(), &[("filter.wgsl", SHADER), ("FILTER.wgsl", SHADER)]);
        assert_eq!(refusal(&cased), Refusal::Unsafe);
    }

    #[test]
    fn a_header_that_disagrees_with_the_directory_is_refused() {
        let mut bytes = a_filter_file(&a_filter());
        let header = bytes.windows(11).position(|w| w == b"filter.wgsl").unwrap();
        bytes[header] = b'g';
        assert_eq!(refusal(&bytes), Refusal::Unsafe);
    }

    #[test]
    fn a_damaged_file_is_refused() {
        let mut bytes = pack(&a_filter(), &[("filter.wgsl", SHADER)]);
        // the first byte of the shader's data
        let header = bytes.windows(11).position(|w| w == b"filter.wgsl").unwrap();
        bytes[header + 12] ^= 0xFF;
        assert_eq!(refusal(&bytes), Refusal::Damaged);
    }

    #[test]
    fn duplicates_are_refused() {
        // one control twice
        let control = a_filter()["controls"][0].clone();
        let twice = with(a_filter(), "/controls", json!([control, control]));
        assert_eq!(refusal(&a_filter_file(&twice)), Refusal::Described);
        // a field twice: two readers could believe either
        let text =
            a_filter().to_string().replacen("\"permissions\":[]", "\"permissions\":[],\"permissions\":[\"net\"]", 1);
        assert_eq!(refusal(&pack_text(text.as_bytes(), &[("filter.wgsl", SHADER)])), Refusal::Described);
        // a file twice in the list
        let file = a_filter()["files"][0].clone();
        let twice = with(a_filter(), "/files", json!([file, file]));
        assert_eq!(refusal(&a_filter_file(&twice)), Refusal::Described);
    }

    #[test]
    fn text_that_hides_or_reorders_is_refused() {
        for bad in
            ["evil\u{202E}txt.exe", "zero\u{200B}width", "tag\u{E0041}", "line\nbreak", "bell\u{7}", "soft\u{AD}hyphen"]
        {
            for field in ["/name", "/description", "/publisher/name", "/controls/0/label"] {
                assert_eq!(
                    refusal(&a_filter_file(&with(a_filter(), field, json!(bad)))),
                    Refusal::Described,
                    "{field}"
                );
            }
        }
        // what isn't English is fine
        let named = with(a_filter(), "/name", json!("フィルター Überlagern"));
        assert!(read_package(&a_filter_file(&named)).is_ok());
        for long in ["/name", "/publisher/name"] {
            let text = "x".repeat(61);
            assert_eq!(refusal(&a_filter_file(&with(a_filter(), long, json!(text)))), Refusal::Described);
        }
        let empty = with(a_filter(), "/name", json!("   "));
        assert_eq!(refusal(&a_filter_file(&empty)), Refusal::Described);
    }

    fn slider_of(package: &Package) -> PluginSlider {
        match &package.manifest.controls[0] {
            PluginControl::Slider { control } => control.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_start_out_of_range_comes_in() {
        let high = with(a_filter(), "/controls/0/starts", json!(40));
        assert_eq!(slider_of(&read_package(&a_filter_file(&high)).unwrap()).starts, 1.0);
        let low = with(a_filter(), "/controls/0/starts", json!(-3));
        let slider = slider_of(&read_package(&a_filter_file(&low)).unwrap());
        assert_eq!((slider.starts, slider.value), (0.0, 0.0));
        let between = with(a_filter(), "/controls/0/starts", json!(0.504));
        assert!((slider_of(&read_package(&a_filter_file(&between)).unwrap()).starts - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_range_that_makes_no_sense_is_refused() {
        for (path, value) in [
            ("/controls/0/least", json!(2)),
            ("/controls/0/step", json!(0)),
            ("/controls/0/step", json!(-1)),
            ("/controls/0/step", json!(0.000_001)),
            ("/controls/0/most", json!(1e30)),
            ("/controls/0/label", json!("")),
            ("/controls/0/key", json!("enabled")),
            ("/controls/0/key", json!("with space")),
            ("/controls/0/key", json!("Upper")),
            ("/controls/0/key", json!("../up")),
        ] {
            assert_eq!(
                refusal(&a_filter_file(&with(a_filter(), path, value.clone()))),
                Refusal::Described,
                "{path} {value}"
            );
        }
        let unit = with(
            a_filter(),
            "/controls",
            json!([{
                "key": "a", "type": "slider", "label": "A", "least": 0, "most": 1, "step": 0.5, "unit": "\u{202E}px",
            }]),
        );
        assert_eq!(refusal(&a_filter_file(&unit)), Refusal::Described);
        let at_least = with(
            a_filter(),
            "/controls",
            json!([{
                "key": "a", "type": "slider", "label": "A", "least": 0, "most": 1, "step": 0.5, "at_least": "x\u{200B}",
            }]),
        );
        assert_eq!(refusal(&a_filter_file(&at_least)), Refusal::Described);
    }

    #[test]
    fn more_controls_than_a_page_shows_are_refused() {
        let controls: Vec<Value> =
            (0..=MOST_CONTROLS).map(|i| json!({ "key": format!("k{i}"), "type": "switch", "label": "A" })).collect();
        assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/controls", json!(controls)))), Refusal::Described);
    }

    #[test]
    fn a_newer_format_is_refused_as_newer() {
        assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/schema", json!(2)))), Refusal::Newer);
        // whatever else has changed in it
        assert_eq!(refusal(&pack_text(br#"{"schema": 9, "everything": "different"}"#, &[])), Refusal::Newer);
        assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/kind", json!("tool")))), Refusal::Newer);
        assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/kind", json!("unlock")))), Refusal::Newer);
        let dial = with(a_filter(), "/controls/0/type", json!("dial"));
        assert_eq!(refusal(&a_filter_file(&dial)), Refusal::Newer);
        assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/schema", json!(0)))), Refusal::NotAPlugin);
    }

    #[test]
    fn fields_nobody_knows_are_passed_over() {
        let mut manifest = a_filter().as_object().unwrap().clone();
        manifest.insert("something_new".to_owned(), json!({ "deep": [1, 2, 3] }));
        assert!(read_package(&a_filter_file(&Value::Object(manifest))).is_ok());
    }

    #[test]
    fn asking_for_permissions_is_refused() {
        let asks = with(a_filter(), "/permissions", json!(["network"]));
        assert_eq!(refusal(&a_filter_file(&asks)), Refusal::Permissions);
    }

    #[test]
    fn a_plugin_for_another_app_is_said_to_be() {
        let other = with(a_filter(), "/compat", json!({ "photocraft": { "interface": 1 } }));
        assert_eq!(refusal(&a_filter_file(&other)), Refusal::OtherApp);
    }

    #[test]
    fn an_id_is_plain_and_ours_stay_ours() {
        for bad in ["nodots", "UPPER.case", "a..b", ".a.b", "a.b.", "a/b.c", "a b.c", "-a.b", "con.example", "a.b\\c"] {
            assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/id", json!(bad)))), Refusal::Described, "{bad}");
        }
        let ours = with(a_filter(), "/id", json!("procraft.sneaky"));
        assert_eq!(refusal(&a_filter_file(&ours)), Refusal::Described);
    }

    #[test]
    fn a_version_is_three_numbers() {
        for bad in ["1.0", "1.0.0.0", "1.0.0-beta", "v1.0.0", "1..0", "../1.0.0", "99999999999.0.0"] {
            assert_eq!(refusal(&a_filter_file(&with(a_filter(), "/version", json!(bad)))), Refusal::Described, "{bad}");
        }
        assert!(version_is_older("1.2.3", "1.10.0"));
        assert!(version_is_older("0.0.4-beta", "0.0.5"));
        assert!(!version_is_older("2.0.0", "1.9.9"));
        assert!(!version_is_older("1.0.0", "1.0.0"));
    }

    fn a_module() -> Vec<u8> {
        crate::patch_run::tests::position_module("", "(i64.const 0)")
    }

    fn a_patch_file(manifest: &Value, module: &[u8]) -> Vec<u8> {
        pack(manifest, &[(MODULE, module)])
    }

    #[test]
    fn a_good_patch_is_read() {
        let module = a_module();
        let manifest = patch("org.example.patch", &module);
        let package = read_package(&a_patch_file(&manifest, &module)).unwrap();
        assert_eq!(package.manifest.kind, PluginKind::Patch);
        assert_eq!(package.manifest.hooks, [Hook::Position]);
    }

    #[test]
    fn a_patch_says_what_it_steps_into_and_what_it_is_written_to() {
        let module = a_module();
        let of = |path: &str, value: Value| {
            refusal(&a_patch_file(&with(patch("org.example.patch", &module), path, value), &module))
        };
        // a hook or an interface this app doesn't have is for a newer one
        assert_eq!(of("/hooks", json!(["position", "layers"])), Refusal::Newer);
        assert_eq!(of("/compat/procraft/interface", json!(2)), Refusal::Newer);
        assert_eq!(of("/hooks", json!([])), Refusal::Described);
        assert_eq!(of("/hooks", json!(["position", "position"])), Refusal::Described);
        assert_eq!(of("/compat/procraft", json!({ "min_app": "0.0.1" })), Refusal::Described);
        // nor may it ask for anything
        assert_eq!(of("/permissions", json!(["files"])), Refusal::Permissions);
        // and a kind that isn't one yet is for a newer one too
        assert_eq!(of("/kind", json!("module")), Refusal::Newer);
    }

    #[test]
    fn a_control_takes_over_a_moved_setting_and_nothing_else() {
        let module = a_module();
        let with_keeps = |keeps: &str| {
            let mut manifest = patch("org.example.patch", &module);
            manifest["controls"] = json!([{ "key": "on", "type": "switch", "label": "On", "keeps": keeps }]);
            a_patch_file(&manifest, &module)
        };
        // (it needs its controls told; here only what it may be kept under is looked at)
        assert_eq!(refusal(&with_keeps("light_interface")), Refusal::Described);
        assert_eq!(refusal(&with_keeps("org.example.other.value")), Refusal::Described);
        assert_eq!(refusal(&with_keeps("eyedropper_layer")), Refusal::Code);
    }

    #[test]
    fn a_control_can_t_be_called_what_a_flag_is_kept_under() {
        let module = a_module();
        let mut manifest = patch("org.example.patch", &module);
        manifest["controls"] = json!([{ "key": "applied", "type": "switch", "label": "On" }]);
        assert_eq!(refusal(&a_patch_file(&manifest, &module)), Refusal::Described);
    }

    #[test]
    fn a_patch_needs_its_module_and_a_module_that_can_run() {
        let module = a_module();
        // listed under another name, or too big by its list
        let mut named = patch("org.example.patch", &module);
        named["files"][0]["path"] = json!("other.wasm");
        assert_eq!(refusal(&pack(&named, &[("other.wasm", &module)])), Refusal::Described);
        let big = with(patch("org.example.patch", &module), "/files/0/size", json!(MOST_MODULE + 1));
        assert_eq!(refusal(&a_patch_file(&big, &module)), Refusal::Described);
        // not code, code that wants things, code with no controls to be told
        let not_code = b"nothing a machine would run".as_slice();
        let manifest = patch("org.example.patch", not_code);
        assert_eq!(refusal(&a_patch_file(&manifest, not_code)), Refusal::Code);
        let wants = crate::patch_run::tests::position_module(r#"(import "env" "open" (func))"#, "(i64.const 0)");
        let manifest = patch("org.example.patch", &wants);
        assert_eq!(refusal(&a_patch_file(&manifest, &wants)), Refusal::Code);
        let mut manifest = patch("org.example.patch", &module);
        manifest["controls"] = json!([{ "key": "on", "type": "switch", "label": "On" }]);
        assert_eq!(refusal(&a_patch_file(&manifest, &module)), Refusal::Code);
    }

    #[test]
    fn a_kept_patch_with_its_module_changed_counts_as_absent() {
        let dir = folder("patch");
        let module = a_module();
        let package = read_package(&a_patch_file(&patch("org.example.patch", &module), &module)).unwrap();
        keep(&dir, &package).unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        assert_eq!(read_code(&dir, "org.example.patch", "1.0.0"), Some((vec![Hook::Position], module)));
        std::fs::write(dir.join("org.example.patch/1.0.0").join(MODULE), b"something else").unwrap();
        assert!(read_all(&dir).is_empty());
        assert_eq!(read_code(&dir, "org.example.patch", "1.0.0"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_filter_needs_its_shader() {
        let wrong = with(a_filter(), "/files/0/path", json!("filter.txt"));
        assert_eq!(refusal(&pack(&wrong, &[("filter.txt", SHADER)])), Refusal::Described);
    }

    /// Bytes from a seed, so a failure can be run again.
    fn scrambled(seed: u64, bytes: &mut [u8]) {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        for _ in 0..8 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = (state as usize) % bytes.len();
            bytes[at] = (state >> 32) as u8;
        }
    }

    #[test]
    fn no_damage_to_a_file_panics_the_reader() {
        let good = a_filter_file(&a_filter());
        for seed in 1..3000 {
            let mut bytes = good.clone();
            scrambled(seed, &mut bytes);
            // anything but a panic: most are refused, a few bytes nobody reads may change nothing
            let _ = read_package(&bytes).map(|p| p.manifest.id);
            let cut = (seed as usize * 7) % good.len();
            let _ = read_package(good.get(..cut).unwrap_or_default()).map(|p| p.manifest.id);
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-plugin-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_is_kept_is_read_back_and_a_bad_file_counts_as_absent() {
        let dir = folder("kept");
        let package = read_package(&a_filter_file(&a_filter())).unwrap();
        let kept = keep(&dir, &package).unwrap();
        assert_eq!(kept, dir.join("org.example.filter").join("1.0.0"));
        assert_eq!(read_all(&dir).len(), 1);
        // a changed byte, then a file more, then one gone: each and it isn't there
        let shader = kept.join("filter.wgsl");
        std::fs::write(&shader, b"changed").unwrap();
        assert!(read_all(&dir).is_empty());
        std::fs::write(&shader, SHADER).unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        std::fs::write(kept.join("extra.txt"), b"x").unwrap();
        assert!(read_all(&dir).is_empty());
        std::fs::remove_file(kept.join("extra.txt")).unwrap();
        std::fs::remove_file(&shader).unwrap();
        assert!(read_all(&dir).is_empty());
        std::fs::write(&shader, SHADER).unwrap();
        // a manifest that says it is something other than where it is kept
        let text = kept.join(MANIFEST);
        let good = std::fs::read(&text).unwrap();
        let other = with(a_filter(), "/version", json!("2.0.0")).to_string();
        std::fs::write(&text, other).unwrap();
        assert!(read_all(&dir).is_empty());
        std::fs::write(&text, good).unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_a_kept_plugin_counts_as_absent() {
        let dir = folder("link");
        let kept = keep(&dir, &read_package(&a_filter_file(&a_filter())).unwrap()).unwrap();
        std::fs::remove_file(kept.join("filter.wgsl")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", kept.join("filter.wgsl")).unwrap();
        assert!(read_all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_a_run_left_behind_goes() {
        let dir = folder("left");
        let staged = dir.join(".staging-1-2");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("half.txt"), b"x").unwrap();
        assert!(read_all(&dir).is_empty());
        assert!(!staged.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_version_replaces_the_old_whole() {
        let dir = folder("replace");
        keep(&dir, &read_package(&a_filter_file(&a_filter())).unwrap()).unwrap();
        let newer = with(a_filter(), "/version", json!("1.1.0"));
        keep(&dir, &read_package(&a_filter_file(&newer)).unwrap()).unwrap();
        let kept = read_all(&dir);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].version, "1.1.0");
        assert!(!dir.join("org.example.filter").join("1.0.0").exists());
        // the same again takes its own place
        keep(&dir, &read_package(&a_filter_file(&newer)).unwrap()).unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        assert!(forget(&dir, "org.example.filter"));
        assert!(read_all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_plain_file_is_read() {
        let dir = folder("plain");
        assert_eq!(read_plugin_file(&dir), None, "a folder");
        assert_eq!(read_plugin_file(&dir.join("missing")), None);
        let file = dir.join("a.procraftplugin");
        std::fs::write(&file, b"hello").unwrap();
        assert_eq!(read_plugin_file(&file).as_deref(), Some(&b"hello"[..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_is_passed_over_rather_than_waited_on() {
        let dir = folder("pipe");
        let pipe = dir.join("a.procraftplugin");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status();
        if made.is_ok_and(|s| s.success()) {
            assert_eq!(read_plugin_file(&pipe), None);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
