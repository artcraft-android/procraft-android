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
    let _ = fs::write(dir.join(STACK_FILE), serde_json::json!({ "name": name }).to_string());
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
            let _ = fs::write(&file, json.to_string());
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
