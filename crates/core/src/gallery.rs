//! The gallery's artworks, the same in every app: one folder each (see project.rs for what is
//! inside) in the app's artworks folder, some in stacks (a folder of them, named in stack.json).
//! Each folder keeps the order of what is in it in order.txt: new folders go first, newest first,
//! and keep their place from then on, so changing an artwork doesn't move it; only a drag does.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const ARTWORK_FILE: &str = "artwork.json";
const STACK_FILE: &str = "stack.json";
/// The folders' order, one name a line, first first.
const ORDER_FILE: &str = "order.txt";
const UNTITLED: &str = "Untitled Artwork";
const STACK: &str = "Stack";
/// The stack the gallery's recovery puts what it finds in.
const RECOVERED: &str = "Recovered Artworks";

/// A saved artwork: its folder, its name and size, and when it was last changed (milliseconds
/// since 1970).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GalleryArtwork {
    pub dir: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub modified_ms: i64,
}

/// Something the gallery shows: an artwork, or a stack of them (in their own order).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum GalleryEntry {
    Artwork { artwork: GalleryArtwork },
    Stack { dir: String, name: String, artworks: Vec<GalleryArtwork> },
}

impl GalleryEntry {
    fn dir(&self) -> &str {
        match self {
            GalleryEntry::Artwork { artwork } => &artwork.dir,
            GalleryEntry::Stack { dir, .. } => dir,
        }
    }
}

/// The order of the folders `present` (name and time changed), given the order `saved` before:
/// those not placed yet first, newest first, then the rest as they were.
fn arrange(present: &[(String, i64)], saved: &[String]) -> Vec<String> {
    let mut placed: Vec<String> = Vec::new();
    for name in saved {
        if present.iter().any(|(p, _)| p == name) && !placed.contains(name) {
            placed.push(name.clone());
        }
    }
    let mut new: Vec<&(String, i64)> = present.iter().filter(|(p, _)| !placed.contains(p)).collect();
    new.sort_by_key(|p| std::cmp::Reverse(p.1));
    new.into_iter().map(|(p, _)| p.clone()).chain(placed).collect()
}

/// `order` with `name` taken out and put back at `to` (its place in the order without it).
fn moved(order: &[String], name: &str, to: i64) -> Vec<String> {
    let mut rest: Vec<String> = order.iter().filter(|n| *n != name).cloned().collect();
    let at = to.clamp(0, rest.len() as i64) as usize;
    rest.insert(at, name.to_owned());
    rest
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn modified(path: &Path) -> i64 {
    fs::metadata(path).and_then(|m| m.modified()).map_or(0, millis)
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn folders(at: &Path) -> Vec<PathBuf> {
    fs::read_dir(at).map_or_else(
        |_| Vec::new(),
        |entries| entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
    )
}

/// A folder name no other has: 128 random bits written as a UUID.
fn fresh_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let half = |k: u8| {
        // each RandomState is keyed afresh
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u8(k);
        h.finish()
    };
    let (a, b) = (half(0), half(1));
    // the version (4) and variant bits of a random UUID
    let a = (a & !0xF000) | 0x4000;
    let b = (b & !(0xC << 60)) | (0x8 << 60);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

/// The folder `from` copied to `to`, the file named `last` in it (if there is one) after the rest.
fn copy_dir(from: &Path, to: &Path, last: Option<&str>) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target, None)?;
        } else if last.is_none_or(|last| entry.file_name() != last) {
            fs::copy(entry.path(), target)?;
        }
    }
    if let Some(last) = last.map(|l| from.join(l)).filter(|l| l.is_file()) {
        fs::copy(&last, to.join(name_of(&last)))?;
    }
    Ok(())
}

fn read_artwork(dir: &Path) -> Option<GalleryArtwork> {
    let file = dir.join(ARTWORK_FILE);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&file).ok()?).ok()?;
    let document = json.get("document")?;
    let side = |k: &str| document.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    Some(GalleryArtwork {
        dir: dir.to_string_lossy().into_owned(),
        name: json.get("name").and_then(serde_json::Value::as_str).unwrap_or(UNTITLED).to_owned(),
        width: side("width"),
        height: side("height"),
        modified_ms: modified(&file),
    })
}

/// Mends an artwork folder whose artwork file is gone or unreadable from a whole copy a save cut
/// short left beside it (`artwork.<n>.tmp`, the newest that reads), the unreadable one kept as
/// `artwork.json.damaged`. True if it reads now.
fn mend_artwork(dir: &Path) -> bool {
    if read_artwork(dir).is_some() {
        return true;
    }
    let whole = |p: &Path| {
        fs::read(p)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .is_some_and(|j| j.get("document").is_some())
    };
    let left: Option<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    let name = name_of(p);
                    name.starts_with("artwork.") && name.ends_with(".tmp") && p.is_file() && whole(p)
                })
                .max_by_key(|p| modified(p))
        })
        .ok()
        .flatten();
    let Some(left) = left else { return false };
    let file = dir.join(ARTWORK_FILE);
    if file.exists() {
        let _ = fs::rename(&file, dir.join(format!("{ARTWORK_FILE}.damaged")));
    }
    fs::rename(&left, &file).is_ok() && read_artwork(dir).is_some()
}

fn write_order(at: &Path, order: &[String]) {
    let file = at.join(ORDER_FILE);
    let next = at.join(format!("{ORDER_FILE}.new"));
    let text: String = order.iter().map(|n| format!("{n}\n")).collect();
    if fs::write(&next, text).is_ok() && fs::rename(&next, &file).is_err() {
        let _ = fs::remove_file(&file);
        let _ = fs::rename(&next, &file);
    }
}

fn write_stack_name(dir: &Path, name: &str) {
    let name = if name.trim().is_empty() { STACK } else { name.trim() };
    let text = serde_json::json!({ "name": name }).to_string();
    let _ = crate::project::write_atomically(&dir.join(STACK_FILE), |p| {
        fs::write(p, &text).map_err(|e| crate::project::file_error("naming the stack", e))
    });
}

/// The artworks in the app's own storage.
#[derive(uniffi::Object)]
pub struct ArtworkStore {
    root: PathBuf,
}

impl ArtworkStore {
    fn at(&self, folder: Option<String>) -> PathBuf {
        folder.map_or_else(|| self.root.clone(), PathBuf::from)
    }

    fn parent_of(&self, dir: &Path) -> PathBuf {
        dir.parent().map_or_else(|| self.root.clone(), Path::to_path_buf)
    }

    fn read_stack(&self, dir: &Path) -> Option<GalleryEntry> {
        let file = dir.join(STACK_FILE);
        if !file.is_file() {
            return None;
        }
        let artworks: Vec<GalleryArtwork> = self
            .entries(dir)
            .into_iter()
            .filter_map(|e| match e {
                GalleryEntry::Artwork { artwork } => Some(artwork),
                GalleryEntry::Stack { .. } => None,
            })
            .collect();
        // an empty stack goes away
        if artworks.is_empty() {
            let _ = fs::remove_dir_all(dir);
            return None;
        }
        let json: Option<serde_json::Value> = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let name = json.as_ref().and_then(|j| j.get("name")).and_then(serde_json::Value::as_str).unwrap_or(STACK);
        Some(GalleryEntry::Stack { dir: dir.to_string_lossy().into_owned(), name: name.to_owned(), artworks })
    }

    /// What is in `at` in its order; in the gallery, stacks too.
    fn entries(&self, at: &Path) -> Vec<GalleryEntry> {
        let dirs = folders(at);
        let top = at == self.root;
        let read: Vec<GalleryEntry> = dirs
            .iter()
            .filter_map(|d| {
                read_artwork(d)
                    .map(|artwork| GalleryEntry::Artwork { artwork })
                    .or_else(|| if top { self.read_stack(d) } else { None })
            })
            .collect();
        let order = Self::order_with(at, &dirs, &read);
        order.iter().filter_map(|n| read.iter().find(|e| name_of(Path::new(e.dir())) == *n).cloned()).collect()
    }

    /// Every folder in `at` in order, those that can't be read this moment (being written) too.
    fn order(&self, at: &Path) -> Vec<String> {
        let dirs = folders(at);
        let read: Vec<GalleryEntry> =
            dirs.iter().filter_map(|d| read_artwork(d).map(|artwork| GalleryEntry::Artwork { artwork })).collect();
        Self::order_with(at, &dirs, &read)
    }

    /// The order of the folders `dirs` in `at` (`read` those read), kept up to date on disk.
    fn order_with(at: &Path, dirs: &[PathBuf], read: &[GalleryEntry]) -> Vec<String> {
        let saved: Vec<String> = fs::read_to_string(at.join(ORDER_FILE))
            .map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        // a folder being written keeps its place; if new, its folder's time places it
        let present: Vec<(String, i64)> = dirs
            .iter()
            .map(|d| {
                let when = read.iter().find(|e| Path::new(e.dir()) == d).map_or_else(
                    || modified(d),
                    |e| match e {
                        GalleryEntry::Artwork { artwork } => artwork.modified_ms,
                        GalleryEntry::Stack { artworks, .. } => {
                            artworks.iter().map(|a| a.modified_ms).max().unwrap_or_else(|| modified(d))
                        }
                    },
                );
                (name_of(d), when)
            })
            .collect();
        let order = arrange(&present, &saved);
        if order != saved {
            write_order(at, &order);
        }
        order
    }

    /// A new stack in `at` holding `artworks` (moved into it), where `place` was: by default the
    /// first of them, and they keep the gallery's order; with `place`, in the order given.
    fn make_stack(&self, artworks: &[PathBuf], place: Option<&Path>) -> PathBuf {
        let before = self.order(&self.root);
        let names: Vec<String> = artworks.iter().map(|a| name_of(a)).collect();
        let inside: Vec<String> = if place.is_some() {
            names.clone()
        } else {
            before.iter().filter(|n| names.contains(n)).cloned().collect()
        };
        let first = place.map(name_of).or_else(|| inside.first().cloned());
        let at = first.and_then(|f| before.iter().position(|n| *n == f)).unwrap_or(0);
        let dir = self.root.join(fresh_name());
        let _ = fs::create_dir_all(&dir);
        write_stack_name(&dir, STACK);
        for a in artworks {
            let _ = fs::rename(a, dir.join(name_of(a)));
        }
        write_order(&dir, &inside);
        let mut rest: Vec<String> = before.iter().filter(|n| !names.contains(n)).cloned().collect();
        rest.push(name_of(&dir));
        let to = before.iter().take(at).filter(|n| !names.contains(n)).count();
        write_order(&self.root, &moved(&rest, &name_of(&dir), to as i64));
        dir
    }
}

#[uniffi::export]
impl ArtworkStore {
    /// The artworks kept in `root` (made if it isn't there).
    #[uniffi::constructor]
    pub fn open(root: String) -> Arc<ArtworkStore> {
        let root = PathBuf::from(root);
        let _ = fs::create_dir_all(&root);
        Arc::new(ArtworkStore { root })
    }

    pub fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// What is in `folder` (a stack's, or none for the gallery) in its order; empty stacks go
    /// away.
    pub fn list(&self, folder: Option<String>) -> Vec<GalleryEntry> {
        self.entries(&self.at(folder))
    }

    /// The artwork changed last, in the gallery or a stack (each is saved when it is left).
    pub fn most_recent(&self) -> Option<GalleryArtwork> {
        self.list(None)
            .into_iter()
            .flat_map(|e| match e {
                GalleryEntry::Artwork { artwork } => vec![artwork],
                GalleryEntry::Stack { artworks, .. } => artworks,
            })
            .max_by_key(|a| a.modified_ms)
    }

    /// A new, empty folder for an artwork in `folder` (a stack's, or none for the gallery).
    pub fn new_folder(&self, folder: Option<String>) -> String {
        let dir = self.at(folder).join(fresh_name());
        let _ = fs::create_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    pub fn delete(&self, dir: String) {
        let _ = fs::remove_dir_all(dir);
    }

    /// A copy of the artwork in `dir`, under the same name, right after it; returns its folder.
    pub fn duplicate(&self, dir: String) -> String {
        let dir = PathBuf::from(dir);
        let at = self.parent_of(&dir);
        let before = self.order(&at);
        let copy = at.join(fresh_name());
        // the artwork's own file last: a copy half made isn't an artwork in the gallery yet
        let _ = copy_dir(&dir, &copy, Some(ARTWORK_FILE));
        // not worked on yet: as recent as the original
        let when = fs::metadata(dir.join(ARTWORK_FILE)).and_then(|m| m.modified());
        if let (Ok(when), Ok(file)) = (when, fs::File::options().write(true).open(copy.join(ARTWORK_FILE))) {
            let _ = file.set_modified(when);
        }
        let place = before.iter().position(|n| *n == name_of(&dir)).map_or(0, |i| i + 1);
        let mut order = before;
        order.push(name_of(&copy));
        write_order(&at, &moved(&order, &name_of(&copy), place as i64));
        copy.to_string_lossy().into_owned()
    }

    /// The artwork or stack in `dir` moved to `to` (its place in the order without it) in
    /// `into`: the gallery (none) or a stack's folder, by default the one it is in; from a stack
    /// to the gallery, it is taken out of it.
    pub fn move_entry(&self, dir: String, to: i64, into: Option<String>) {
        let dir = PathBuf::from(dir);
        let into = into.map_or_else(|| self.parent_of(&dir), PathBuf::from);
        if dir.parent() != Some(into.as_path()) && fs::rename(&dir, into.join(name_of(&dir))).is_err() {
            return;
        }
        write_order(&into, &moved(&self.order(&into), &name_of(&dir), to));
    }

    /// The artwork in `dir` (from the gallery) dropped onto `onto`: an artwork makes a stack with
    /// it there, this one first; a stack takes it in first.
    pub fn put_onto(&self, dir: String, onto: String) {
        let (dir, onto) = (PathBuf::from(dir), PathBuf::from(onto));
        if !onto.join(STACK_FILE).is_file() {
            self.make_stack(&[dir, onto.clone()], Some(&onto));
            return;
        }
        let inside = self.order(&onto);
        if fs::rename(&dir, onto.join(name_of(&dir))).is_err() {
            return;
        }
        let mut order = vec![name_of(&dir)];
        order.extend(inside);
        write_order(&onto, &order);
    }

    /// The artwork in `dir` named `name`.
    pub fn rename(&self, dir: String, name: String) {
        let file = Path::new(&dir).join(ARTWORK_FILE);
        let Some(mut json) = fs::read(&file).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            return;
        };
        if let Some(o) = json.as_object_mut() {
            o.insert("name".into(), name.trim().into());
            // written whole or not at all: cut short, the artwork couldn't be opened
            let text = json.to_string();
            let _ = crate::project::write_atomically(&file, |p| {
                fs::write(p, &text).map_err(|e| crate::project::file_error("renaming the artwork", e))
            });
        }
    }

    /// A new stack in the gallery holding the artworks in `dirs` (moved into it), where the first
    /// of them was, in the gallery's order; returns its folder.
    pub fn stack(&self, dirs: Vec<String>) -> String {
        let artworks: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
        self.make_stack(&artworks, None).to_string_lossy().into_owned()
    }

    /// The stack in `dir` named `name` ("Stack" if it is empty).
    pub fn rename_stack(&self, dir: String, name: String) {
        write_stack_name(Path::new(&dir), &name);
    }

    /// Looks for artworks the gallery can't show and puts them in a new stack, "Recovered
    /// Artworks", first in the gallery: artworks in a folder that is neither an artwork nor a
    /// stack (a stack that lost its name file) or kept deeper than the gallery looks, and
    /// artworks whose file is gone or unreadable but which keep a whole one from a save cut short.
    /// Returns how many it found.
    pub fn recover_lost_artworks(&self) -> u32 {
        let shown: Vec<PathBuf> = self
            .list(None)
            .iter()
            .flat_map(|e| match e {
                GalleryEntry::Artwork { artwork } => vec![PathBuf::from(&artwork.dir)],
                GalleryEntry::Stack { artworks, .. } => artworks.iter().map(|a| PathBuf::from(&a.dir)).collect(),
            })
            .collect();
        // every folder down to three deep: one not shown that holds an artwork (mended if it can
        // be) is lost; the others are looked into
        let mut lost: Vec<PathBuf> = Vec::new();
        let mut look = vec![(self.root.clone(), 0)];
        while let Some((at, depth)) = look.pop() {
            for dir in folders(&at) {
                if !shown.contains(&dir) && mend_artwork(&dir) {
                    lost.push(dir);
                } else if depth < 2 {
                    look.push((dir, depth + 1));
                }
            }
        }
        if lost.is_empty() {
            return 0;
        }
        let stack = self.root.join(fresh_name());
        if fs::create_dir_all(&stack).is_err() {
            return 0;
        }
        write_stack_name(&stack, RECOVERED);
        let moved = lost.iter().filter(|a| fs::rename(a, stack.join(name_of(a))).is_ok()).count();
        // first in the gallery
        let order: Vec<String> = std::iter::once(name_of(&stack)).chain(self.order(&self.root)).collect();
        write_order(&self.root, &order);
        moved as u32
    }
}

/// What a zip file holds, told from its names alone (read from the file's end, so a big artwork
/// isn't read to find out): how Import takes it.
#[derive(uniffi::Enum, Debug, PartialEq, Eq)]
pub enum ZipKind {
    /// A model's artwork, whole.
    ModelArtwork,
    /// Not an archive that can be read.
    NotZip,
    Brushes,
    Swatches,
    /// A model, at `name` in the zip, its files beside it.
    Model {
        name: String,
    },
    /// An artwork file of ours, a layered package, or anything else.
    Other,
}

#[uniffi::export]
pub fn zip_file_kind(path: String) -> ZipKind {
    let Some(names) = procraft_engine::zip::names_in_file(Path::new(&path)) else { return ZipKind::NotZip };
    let names = || names.iter().map(String::as_str);
    if crate::brush_files::brush_names(names()) {
        ZipKind::Brushes
    } else if names().any(|n| n.rsplit('/').next() == Some(procraft_engine::palette::SWATCHES_ENTRY)) {
        ZipKind::Swatches
    } else if crate::scene3d::is_model_artwork(names()) {
        ZipKind::ModelArtwork
    } else if let Some(name) = crate::scene3d::model_name_among(names()) {
        ZipKind::Model { name }
    } else {
        ZipKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_file_is_told_by_its_names() {
        let dir = std::env::temp_dir().join(format!("procraft-zip-kind-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let zip = |file: &str, entries: &[&str]| {
            let mut w = procraft_engine::zip::ZipWriter::new();
            for e in entries {
                w.add(e, b"x");
            }
            let path = dir.join(file);
            fs::write(&path, w.finish()).unwrap();
            zip_file_kind(path.to_string_lossy().into_owned())
        };
        let kinds = [
            zip("set.zip", &["brushset.plist", "a/Brush.archive"]),
            zip("one.zip", &["b/Brush.archive", "b/Shape.png"]),
            zip("colours.zip", &["Swatches.json"]),
            zip("model.zip", &["__MACOSX/._chair.obj", "chair/chair.obj", "chair/chair.mtl"]),
            zip("art.zip", &["Document.archive", "chair.obj", "QuickLook/Thumbnail.png"]),
            zip("other.zip", &["notes.txt"]),
        ];
        fs::write(dir.join("plain.zip"), b"PK not really").unwrap();
        let plain = zip_file_kind(dir.join("plain.zip").to_string_lossy().into_owned());
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            kinds,
            [
                ZipKind::Brushes,
                ZipKind::Brushes,
                ZipKind::Swatches,
                ZipKind::Model { name: "chair/chair.obj".to_owned() },
                ZipKind::Other,
                ZipKind::Other,
            ]
        );
        assert_eq!(plain, ZipKind::NotZip);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn present(v: &[(&str, i64)]) -> Vec<(String, i64)> {
        v.iter().map(|(n, t)| ((*n).to_owned(), *t)).collect()
    }

    #[test]
    fn with_no_order_yet_the_newest_come_first() {
        assert_eq!(arrange(&present(&[("a", 10), ("b", 30), ("c", 20)]), &[]), names(&["b", "c", "a"]));
    }

    #[test]
    fn a_changed_artwork_keeps_its_place_and_a_new_one_goes_first() {
        // "a" was changed last, but it was placed already; "d" is new
        let now = present(&[("a", 99), ("b", 30), ("c", 20), ("d", 50)]);
        assert_eq!(arrange(&now, &names(&["c", "a", "b"])), names(&["d", "c", "a", "b"]));
    }

    #[test]
    fn gone_folders_and_repeats_leave_the_order() {
        let now = present(&[("a", 1), ("b", 2)]);
        assert_eq!(arrange(&now, &names(&["b", "x", "a", "b"])), names(&["b", "a"]));
    }

    #[test]
    fn a_move_takes_the_entry_out_and_puts_it_back_at_its_new_place() {
        let order = names(&["a", "b", "c", "d"]);
        assert_eq!(moved(&order, "a", 2), names(&["b", "c", "a", "d"]));
        assert_eq!(moved(&order, "d", 0), names(&["d", "a", "b", "c"]));
        // past either end it goes to that end
        assert_eq!(moved(&order, "a", 99), names(&["b", "c", "d", "a"]));
        assert_eq!(moved(&order, "c", -1), names(&["c", "a", "b", "d"]));
        // a name not there yet is put in
        assert_eq!(moved(&order, "e", 1), names(&["a", "e", "b", "c", "d"]));
    }

    #[test]
    fn fresh_names_differ() {
        let (a, b) = (fresh_name(), fresh_name());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }

    fn artwork(store: &ArtworkStore, name: &str, folder: Option<String>) -> String {
        let dir = store.new_folder(folder);
        let json = serde_json::json!({ "name": name, "document": { "width": 64, "height": 32 } });
        fs::write(Path::new(&dir).join(ARTWORK_FILE), json.to_string()).unwrap();
        dir
    }

    fn shown(store: &ArtworkStore, folder: Option<String>) -> Vec<String> {
        store
            .list(folder)
            .into_iter()
            .map(|e| match e {
                GalleryEntry::Artwork { artwork } => artwork.name,
                GalleryEntry::Stack { name, artworks, .. } => {
                    format!("{name}[{}]", artworks.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(","))
                }
            })
            .collect()
    }

    #[test]
    fn recovery_stacks_artworks_the_gallery_cannot_show() {
        let root = std::env::temp_dir().join(format!("procraft-recover-{}-{}", std::process::id(), fresh_name()));
        let store = ArtworkStore::open(root.to_string_lossy().into_owned());
        artwork(&store, "Shown", None);
        // nothing lost: nothing done
        assert_eq!(store.recover_lost_artworks(), 0);
        assert_eq!(shown(&store, None), ["Shown"]);
        // a stack that lost its name file
        let stack = store.stack(vec![artwork(&store, "A", None), artwork(&store, "B", None)]);
        fs::remove_file(Path::new(&stack).join(STACK_FILE)).unwrap();
        // one kept inside a stack's artwork, too deep to be shown
        let kept = store.stack(vec![artwork(&store, "Outer", None)]);
        let outer = store.list(Some(kept.clone()))[0].dir().to_owned();
        artwork(&store, "Deep", Some(outer));
        // a first save cut short: no artwork file, a whole one left beside it
        let cut = store.new_folder(None);
        let json = serde_json::json!({ "name": "Cut", "document": { "width": 8, "height": 8 } });
        fs::write(Path::new(&cut).join("artwork.3.tmp"), json.to_string()).unwrap();
        // a save cut short mid-write leaves a part of one: nothing to mend
        let torn = store.new_folder(None);
        fs::write(Path::new(&torn).join("artwork.4.tmp"), "{\"name\": \"Tor").unwrap();
        let mut before = shown(&store, None);
        before.sort();
        assert_eq!(before, ["Shown", "Stack[Outer]"]);
        assert_eq!(store.recover_lost_artworks(), 4);
        let after = shown(&store, None);
        assert_eq!(after.len(), 3);
        // the stack first, holding them all
        assert!(after[0].starts_with("Recovered Artworks["), "{after:?}");
        let mut found: Vec<&str> =
            after[0].trim_start_matches("Recovered Artworks[").trim_end_matches(']').split(',').collect();
        found.sort_unstable();
        assert_eq!(found, ["A", "B", "Cut", "Deep"]);
        assert!(Path::new(&torn).join("artwork.4.tmp").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gallery_work_at_random_loses_no_artwork_and_never_nests_a_stack() {
        for seed in 0u64..20 {
            let root =
                std::env::temp_dir().join(format!("procraft-gallery-fuzz-{}-{}", std::process::id(), fresh_name()));
            let store = ArtworkStore::open(root.to_string_lossy().into_owned());
            let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = |n: usize| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % n.max(1) as u64) as usize
            };
            // every artwork's name, as many times as there are artworks of it
            let mut names: Vec<String> = Vec::new();
            let mut made = 0;
            let mut done: Vec<usize> = Vec::new();
            for _ in 0..40 {
                let gallery = store.list(None);
                let stacks: Vec<(String, Vec<GalleryArtwork>)> = gallery
                    .iter()
                    .filter_map(|e| match e {
                        GalleryEntry::Stack { dir, artworks, .. } => Some((dir.clone(), artworks.clone())),
                        GalleryEntry::Artwork { .. } => None,
                    })
                    .collect();
                let loose: Vec<String> = gallery
                    .iter()
                    .filter_map(|e| match e {
                        GalleryEntry::Artwork { artwork } => Some(artwork.dir.clone()),
                        GalleryEntry::Stack { .. } => None,
                    })
                    .collect();
                let stacked: Vec<String> = stacks.iter().flat_map(|(_, a)| a.iter().map(|x| x.dir.clone())).collect();
                let all: Vec<String> = loose.iter().chain(&stacked).cloned().collect();
                let op = next(10);
                done.push(op);
                match op {
                    // a new artwork, in the gallery or in a stack
                    0 | 1 => {
                        made += 1;
                        let name = format!("a{made}");
                        let folder =
                            if next(3) == 0 { stacks.get(next(stacks.len())).map(|s| s.0.clone()) } else { None };
                        artwork(&store, &name, folder);
                        names.push(name);
                    }
                    2 if !all.is_empty() => {
                        let dir = &all[next(all.len())];
                        let name = name_in(dir);
                        store.duplicate(dir.clone());
                        names.push(name);
                    }
                    3 if !all.is_empty() => {
                        let dir = &all[next(all.len())];
                        let name = name_in(dir);
                        store.delete(dir.clone());
                        let at = names.iter().position(|n| *n == name).unwrap();
                        names.remove(at);
                    }
                    // moved within the gallery or within its stack
                    4 if !gallery.is_empty() => {
                        let dir = gallery[next(gallery.len())].dir().to_owned();
                        store.move_entry(dir, next(gallery.len() + 1) as i64, None);
                    }
                    5 if !stacked.is_empty() => {
                        let dir = stacked[next(stacked.len())].clone();
                        let to = if next(2) == 0 { Some(store.root()) } else { None };
                        store.move_entry(dir, next(gallery.len() + 2) as i64, to);
                    }
                    // an artwork of the gallery dropped onto another of its entries
                    6 if !loose.is_empty() && gallery.len() > 1 => {
                        let dir = loose[next(loose.len())].clone();
                        let onto = gallery[next(gallery.len())].dir().to_owned();
                        if onto != dir {
                            store.put_onto(dir, onto);
                        }
                    }
                    7 if loose.len() > 1 => {
                        let picked: Vec<String> = loose.iter().filter(|_| next(2) == 0).cloned().collect();
                        if !picked.is_empty() {
                            store.stack(picked);
                        }
                    }
                    8 if !all.is_empty() => {
                        let dir = &all[next(all.len())];
                        let old = name_in(dir);
                        made += 1;
                        let name = format!("a{made}");
                        store.rename(dir.clone(), name.clone());
                        let at = names.iter().position(|n| *n == old).unwrap();
                        names[at] = name;
                    }
                    9 if !stacks.is_empty() => store.rename_stack(stacks[next(stacks.len())].0.clone(), "S".into()),
                    _ => {}
                }
                // what is listed is every artwork there is, each once, and no stack empty
                let mut listed: Vec<String> = Vec::new();
                let mut dirs: Vec<String> = Vec::new();
                for e in store.list(None) {
                    match e {
                        GalleryEntry::Artwork { artwork } => {
                            listed.push(artwork.name);
                            dirs.push(artwork.dir);
                        }
                        GalleryEntry::Stack { artworks, .. } => {
                            assert!(!artworks.is_empty(), "seed {seed}: an empty stack after {done:?}");
                            for a in artworks {
                                listed.push(a.name);
                                dirs.push(a.dir);
                            }
                        }
                    }
                }
                let mut want = names.clone();
                want.sort();
                listed.sort();
                assert_eq!(listed, want, "seed {seed}: after {done:?}");
                // on disk: each artwork's folder listed once, no stack within a stack
                let mut on_disk: Vec<String> = Vec::new();
                for entry in fs::read_dir(&root).unwrap().flatten() {
                    let path = entry.path();
                    if path.join(ARTWORK_FILE).is_file() {
                        on_disk.push(path.to_string_lossy().into_owned());
                    } else if path.join(STACK_FILE).is_file() {
                        for inner in fs::read_dir(&path).unwrap().flatten() {
                            let inner = inner.path();
                            assert!(
                                !inner.join(STACK_FILE).is_file(),
                                "seed {seed}: a stack in a stack after {done:?}"
                            );
                            if inner.join(ARTWORK_FILE).is_file() {
                                on_disk.push(inner.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
                on_disk.sort();
                dirs.sort();
                let canon = |v: &[String]| {
                    v.iter().map(|d| PathBuf::from(d).components().collect::<PathBuf>()).collect::<Vec<_>>()
                };
                assert_eq!(canon(&dirs), canon(&on_disk), "seed {seed}: after {done:?}");
            }
            let _ = fs::remove_dir_all(&root);
        }
    }

    /// The name kept in the artwork's file in `dir`.
    fn name_in(dir: &str) -> String {
        let text = fs::read_to_string(Path::new(dir).join(ARTWORK_FILE)).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["name"].as_str().unwrap().to_owned()
    }

    #[test]
    fn artworks_are_listed_stacked_moved_duplicated_and_renamed() {
        let root = std::env::temp_dir().join(format!("procraft-gallery-{}-{}", std::process::id(), fresh_name()));
        let store = ArtworkStore::open(root.to_string_lossy().into_owned());
        let a = artwork(&store, "A", None);
        let b = artwork(&store, "B", None);
        let c = artwork(&store, "C", None);
        // placed newest first (they may share a time: put them in a known order)
        store.move_entry(a.clone(), 0, None);
        store.move_entry(b.clone(), 1, None);
        store.move_entry(c.clone(), 2, None);
        assert_eq!(shown(&store, None), names(&["A", "B", "C"]));
        let list = store.list(None);
        assert!(
            matches!(&list[0], GalleryEntry::Artwork { artwork } if artwork.width == 64 && artwork.height == 32),
            "{list:?}"
        );
        // a duplicate goes right after its original, with all its files
        fs::create_dir_all(Path::new(&a).join("layers")).unwrap();
        fs::write(Path::new(&a).join("layers").join("1.bin"), [7u8; 3]).unwrap();
        let copy = store.duplicate(a.clone());
        assert_eq!(shown(&store, None), names(&["A", "A", "B", "C"]));
        assert_eq!(fs::read(Path::new(&copy).join("layers").join("1.bin")).unwrap(), [7u8; 3]);
        assert!(Path::new(&copy).join(ARTWORK_FILE).is_file());
        // C dropped onto B makes a stack where B was, C first
        store.put_onto(c.clone(), b.clone());
        let stack = match &store.list(None)[2] {
            GalleryEntry::Stack { dir, .. } => dir.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(shown(&store, None), names(&["A", "A", "Stack[C,B]"]));
        store.rename_stack(stack.clone(), "Mine".into());
        store.rename(a.clone(), "  First ".into());
        assert_eq!(shown(&store, None), names(&["First", "A", "Mine[C,B]"]));
        // out of the stack to the front of the gallery
        let c_now = Path::new(&stack).join(name_of(Path::new(&c))).to_string_lossy().into_owned();
        store.move_entry(c_now, 0, Some(store.root()));
        assert_eq!(shown(&store, None), names(&["C", "First", "A", "Mine[B]"]));
        // the last one out, the stack goes
        let b_now = Path::new(&stack).join(name_of(Path::new(&b))).to_string_lossy().into_owned();
        store.move_entry(b_now, 4, Some(store.root()));
        assert_eq!(shown(&store, None), names(&["C", "First", "A", "B"]));
        assert!(!Path::new(&stack).exists());
        // two picked make a stack where the first of them was, in the gallery's order
        let list = store.list(None);
        let dir = |i: usize| list[i].dir().to_owned();
        store.stack(vec![dir(3), dir(1)]);
        assert_eq!(shown(&store, None), names(&["C", "Stack[First,B]", "A"]));
        assert!(store.most_recent().is_some());
        let _ = fs::remove_dir_all(&root);
    }
}
