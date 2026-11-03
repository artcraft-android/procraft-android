//! Brush libraries: the one in use is where the brushes come from (its sets in the state); each
//! library is kept in files of its own in the brushes' folder, the first in the files a single
//! library always had (so a single library's folder is as it always was), and a small index says
//! their order, their names and which is in use. They share the folders their brushes' pictures and
//! grains are kept in. Bringing another into use keeps the one in use in its files first, then only
//! the index changes: cut short anywhere, the libraries are all still there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_engine::brush::Brush;

use crate::brush_library::{LibrarySet, library_writes};
use crate::layers::ThumbnailImage;
use crate::{BrushInfo, BrushSetInfo, CanvasSession};

/// How many libraries not in use have their brushes kept read, for their cards' previews.
const READ_KEPT: usize = 4;

/// The library as it ships is called this; a new one is called [`NEW_NAME`], and both are
/// numbered after their name while another has it.
pub(crate) const STOCK_NAME: &str = "Brush library";
const NEW_NAME: &str = "Untitled library";
/// The index of the libraries, in the brushes' folder.
const INDEX_FILE: &str = "libraries.json";
/// The first library's id: its files are the ones a single library always had.
const FIRST: &str = "main";
/// How many times bringing another library into use keeps the one in use again when it changed
/// while it was being kept, before giving up (the one in use stays, as it is).
const KEEP_TRIES: usize = 3;

/// A brush library as its card shows it: its name, whether it is the one in use, and its sets.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct BrushLibraryInfo {
    pub name: String,
    pub current: bool,
    pub sets: Vec<BrushSetInfo>,
}

/// A set as a library's card shows it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct CardSet {
    name: String,
    #[serde(default)]
    icon: u32,
    #[serde(default)]
    brushes: Vec<String>,
    #[serde(default)]
    chosen: Option<u32>,
}

impl CardSet {
    fn of(set: &BrushSetInfo) -> CardSet {
        CardSet {
            name: set.name.clone(),
            icon: set.icon,
            brushes: set.brushes.iter().map(|b| b.name.clone()).collect(),
            chosen: set.chosen,
        }
    }

    fn of_set(set: &LibrarySet) -> CardSet {
        CardSet {
            name: set.name.clone(),
            icon: set.icon,
            brushes: set.brushes.iter().map(|b| b.name.clone()).collect(),
            chosen: None,
        }
    }

    fn info(&self, s: usize) -> BrushSetInfo {
        BrushSetInfo {
            name: self.name.clone(),
            icon: self.icon,
            chosen: self.chosen,
            brushes: self
                .brushes
                .iter()
                .enumerate()
                .map(|(i, name)| BrushInfo { set: s as u32, index: i as u32, name: name.clone() })
                .collect(),
        }
    }
}

/// A library in the index: its files' id, its name, whether shipped brushes join it, and its
/// sets as its card shows them (as they were last kept).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Entry {
    id: String,
    name: String,
    #[serde(default)]
    stock: bool,
    #[serde(default)]
    sets: Vec<CardSet>,
}

impl Entry {
    fn first() -> Entry {
        Entry { id: FIRST.to_owned(), name: STOCK_NAME.to_owned(), stock: true, sets: Vec::new() }
    }
}

/// The index as kept: the libraries in order, and the one in use (its id).
#[derive(serde::Serialize)]
struct IndexFile<'a> {
    libraries: &'a [Entry],
    current: &'a str,
}

/// The libraries: in order, which the app has in use (by id) and which this session holds in the
/// state (the same, unless another session brought another into use since), the folder they are
/// kept in, and the brushes of a few not in use, read from their files (by id, the latest last).
#[derive(Clone, Debug)]
pub(crate) struct Libraries {
    list: Vec<Entry>,
    current: String,
    held: String,
    dir: Option<PathBuf>,
    read: Vec<(String, Arc<Vec<Vec<Brush>>>)>,
}

impl Default for Libraries {
    fn default() -> Libraries {
        Libraries {
            list: vec![Entry::first()],
            current: FIRST.to_owned(),
            held: FIRST.to_owned(),
            dir: None,
            read: Vec::new(),
        }
    }
}

impl Libraries {
    /// Whether shipped brushes join the library held.
    pub(crate) fn stock(&self) -> bool {
        self.list.iter().find(|e| e.id == self.held).is_some_and(|e| e.stock)
    }

    /// The key a QuickMenu button's brush is counted on from in the library held: each library
    /// its own, so that a key one library gave names no brush in another (the first library's
    /// from nothing, as the single library's always were).
    pub(crate) fn first_quick_key(&self) -> u64 {
        number(&self.held).map_or(0, |n| u64::from(n) << 32)
    }

    /// The place of library `id`.
    fn place(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|e| e.id == id)
    }
}

/// `name`, or numbered after it ("Name 2", "Name 3"...) while one of `list` has it.
fn unique_in(list: &[Entry], name: &str) -> String {
    let taken = |n: &str| list.iter().any(|e| e.name == n);
    if !taken(name) {
        return name.to_owned();
    }
    // a numbered name counts on from its number
    let (base, from) = match name.rsplit_once(' ').map(|(b, n)| (b, n.parse::<u32>())) {
        Some((base, Ok(n))) if n >= 2 => (base, n),
        _ => (name, 1),
    };
    (from + 1..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).unwrap_or_else(|| name.to_owned())
}

/// The number of library `id` ("lib-N"); none for the first library's.
fn number(id: &str) -> Option<u32> {
    let n = id.strip_prefix("lib-")?;
    (!n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten()
}

/// Whether `id` is one a library's files can have (it names files: nothing else is taken).
fn is_id(id: &str) -> bool {
    id == FIRST || number(id).is_some()
}

/// Library `id`'s files in `dir`: its sets, and its tools' and sets' choices.
fn files(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    if id == FIRST {
        (dir.join(crate::brush_library::LIBRARY_FILE), dir.join(crate::brush_library::CHOICES_FILE))
    } else {
        (dir.join(format!("{id}.json")), dir.join(format!("{id}.choices.json")))
    }
}

/// An id none of `list` has and no file in `dir` is kept by (one left behind isn't taken over).
fn new_id(list: &[Entry], dir: &Path) -> String {
    (1u32..)
        .map(|n| format!("lib-{n}"))
        .find(|id| list.iter().all(|e| e.id != *id) && !files(dir, id).0.exists() && !files(dir, id).1.exists())
        .unwrap_or_default()
}

/// The index kept in `dir`, as much of it as can be read (a library whose entry can't be is left
/// out, its files kept); none if there is none or it isn't an index at all.
fn read_index(dir: &Path) -> Option<(Vec<Entry>, String)> {
    let text = std::fs::read_to_string(dir.join(INDEX_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let entries = value.get("libraries")?.as_array()?;
    let mut list: Vec<Entry> = Vec::new();
    for entry in entries {
        // an entry whose card can't be read keeps its id and name
        let read = serde_json::from_value::<Entry>(entry.clone()).ok().or_else(|| {
            let id = entry.get("id")?.as_str()?.to_owned();
            let name = entry.get("name").and_then(serde_json::Value::as_str).unwrap_or(NEW_NAME).to_owned();
            let stock = entry.get("stock").and_then(serde_json::Value::as_bool).unwrap_or(false);
            Some(Entry { id, name, stock, sets: Vec::new() })
        });
        let Some(mut read) = read else { continue };
        if !is_id(&read.id) || list.iter().any(|e| e.id == read.id) {
            continue;
        }
        read.name = read.name.trim().chars().take(256).collect();
        if read.name.is_empty() {
            read.name = unique_in(&list, NEW_NAME);
        }
        list.push(read);
    }
    if list.is_empty() {
        return None;
    }
    let current = value.get("current").and_then(serde_json::Value::as_str).unwrap_or(FIRST).to_owned();
    let current = if list.iter().any(|e| e.id == current) {
        current
    } else {
        list.first().map_or_else(|| FIRST.to_owned(), |e| e.id.clone())
    };
    Some((list, current))
}

/// The libraries kept in `dir`: as the index has them; with none to read, the first library and
/// any other whose files are there (an index lost leaves no library behind).
fn libraries_in(dir: &Path) -> (Vec<Entry>, String) {
    if let Some(index) = read_index(dir) {
        return index;
    }
    let mut list = vec![Entry::first()];
    let mut found: Vec<u32> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            number(name.strip_suffix(".json")?)
        })
        .collect();
    found.sort_unstable();
    for n in found {
        let name = unique_in(&list, NEW_NAME);
        list.push(Entry { id: format!("lib-{n}"), name, stock: false, sets: Vec::new() });
    }
    (list, FIRST.to_owned())
}

/// The file of the library in use in `dir`, and whether it is the first library's (whose file an
/// empty library never is).
pub(crate) fn file_in_use(dir: &Path) -> (PathBuf, bool) {
    let (_, current) = libraries_in(dir);
    (files(dir, &current).0, current == FIRST)
}

/// `text` kept in the file at `path`, replaced whole; false if it couldn't be.
fn write(path: &Path, text: &str) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let written = crate::project::write_atomically(path, |p| {
        std::fs::write(p, text.as_bytes()).map_err(|e| crate::project::file_error("keeping the brushes", e))
    });
    if let Err(e) = &written {
        log::error!("{e}");
    }
    written.is_ok()
}

#[uniffi::export]
impl CanvasSession {
    /// Every brush library in order, the one in use among them.
    pub fn brush_libraries(&self) -> Vec<BrushLibraryInfo> {
        let mut current = Some(self.brush_library());
        let st = self.shared.lock();
        let l = &st.libraries;
        l.list
            .iter()
            .map(|e| BrushLibraryInfo {
                name: e.name.clone(),
                current: e.id == l.held,
                sets: match current.take_if(|_| e.id == l.held) {
                    Some(sets) => sets,
                    None => e.sets.iter().enumerate().map(|(s, set)| set.info(s)).collect(),
                },
            })
            .collect()
    }

    /// A brush of the library at `library` drawn as [`CanvasSession::brush_preview`] draws one of
    /// the library in use (another's brushes read from its files once, and kept a while); none if
    /// there is no such brush or it can't be drawn.
    pub fn brush_library_preview(
        &self,
        library: u32,
        set: u32,
        index: u32,
        width: u32,
        height: u32,
    ) -> Option<ThumbnailImage> {
        let (held, id, dir) = {
            let st = self.shared.lock();
            let l = &st.libraries;
            (l.held.clone(), l.list.get(library as usize)?.id.clone(), l.dir.clone())
        };
        if id == held {
            return self.brush_preview(set, index, width, height);
        }
        let brushes = self.library_brushes(&id, &dir?)?;
        let brush = brushes.get(set as usize)?.get(index as usize)?.clone();
        self.drawn_preview(brush, width, height)
    }

    /// Brings the library at `index` into use, the one in use kept in its files first. False,
    /// changing nothing, if there is none there, its files can't be read, the one in use can't be
    /// kept, or the libraries have no folder ([`CanvasSession::open_brushes`] gives them theirs).
    pub fn open_brush_library(&self, index: u32) -> bool {
        let _writing = library_writes();
        let (dir, held, target) = {
            let st = self.shared.lock();
            let l = &st.libraries;
            (l.dir.clone(), l.held.clone(), l.list.get(index as usize).map(|e| e.id.clone()))
        };
        let (Some(dir), Some(target)) = (dir, target) else { return false };
        target == held || self.bring_into_use(&dir, &target)
    }

    /// Renames the library at `index`; false if there is none there, the name is blank, or the
    /// index couldn't be kept.
    pub fn rename_brush_library(&self, index: u32, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        let _writing = library_writes();
        let (dir, id) = {
            let mut st = self.shared.lock();
            let Some(e) = st.libraries.list.get_mut(index as usize) else { return false };
            e.name = name.clone();
            let id = e.id.clone();
            (st.libraries.dir.clone(), id)
        };
        dir.is_none_or(|dir| {
            self.update_index(&dir, |list, _| {
                if let Some(e) = list.iter_mut().find(|e| e.id == id) {
                    e.name = name;
                }
            })
        })
    }

    /// A new, empty library after the others; its place among them, or none if it couldn't be
    /// kept or the libraries have no folder.
    pub fn new_brush_library(&self) -> Option<u32> {
        let _writing = library_writes();
        let dir = self.shared.lock().libraries.dir.clone()?;
        let (list, _) = self.index_now(&dir);
        let id = new_id(&list, &dir);
        let (sets, choices) = files(&dir, &id);
        if !(write(&sets, "[]") && write(&choices, "{}")) {
            return None;
        }
        let entry = Entry { id: id.clone(), name: unique_in(&list, NEW_NAME), stock: false, sets: Vec::new() };
        self.update_index(&dir, |list, _| list.push(entry)).then(|| self.place_of(&id)).flatten()
    }

    /// A copy of the library at `index` just after it, numbered after its name (shipped brushes
    /// don't join a copy); its place, or none if there is no library there or it couldn't be
    /// copied.
    pub fn duplicate_brush_library(&self, index: u32) -> Option<u32> {
        let _writing = library_writes();
        let (dir, held, source) = {
            let st = self.shared.lock();
            let l = &st.libraries;
            (l.dir.clone()?, l.held.clone(), l.list.get(index as usize).cloned()?)
        };
        // the one in use copied as it is now
        let mut card = source.sets.clone();
        if source.id == held {
            if !self.keep_library_in_use(&dir) {
                return None;
            }
            card = self.brush_library().iter().map(CardSet::of).collect();
        }
        let (list, _) = self.index_now(&dir);
        let id = new_id(&list, &dir);
        let (from_sets, from_choices) = files(&dir, &source.id);
        let (to_sets, to_choices) = files(&dir, &id);
        let sets = std::fs::read_to_string(from_sets).ok()?;
        let choices = std::fs::read_to_string(from_choices).unwrap_or_else(|_| "{}".to_owned());
        // (the pictures and grains it names are shared, in the folders beside them all)
        if !(write(&to_sets, &sets) && write(&to_choices, &choices)) {
            return None;
        }
        let entry = Entry { id: id.clone(), name: unique_in(&list, &source.name), stock: false, sets: card };
        let added = self.update_index(&dir, |list, _| {
            let at = list.iter().position(|e| e.id == source.id).map_or(list.len(), |p| p + 1);
            list.insert(at, entry);
        });
        added.then(|| self.place_of(&id)).flatten()
    }

    /// Deletes the library at `index` (the one in use: the next, else the one before it, comes
    /// into use first); false if it is the only one, there is none there, or it couldn't be.
    pub fn delete_brush_library(&self, index: u32) -> bool {
        let _writing = library_writes();
        let index = index as usize;
        let (dir, held, list) = {
            let st = self.shared.lock();
            let l = &st.libraries;
            (l.dir.clone(), l.held.clone(), l.list.clone())
        };
        let Some(dir) = dir else { return false };
        let Some(gone) = list.get(index).map(|e| e.id.clone()).filter(|_| list.len() >= 2) else { return false };
        if gone == held {
            let next = if index + 1 < list.len() { index + 1 } else { index.saturating_sub(1) };
            let Some(next) = list.get(next).map(|e| e.id.clone()) else { return false };
            if !self.bring_into_use(&dir, &next) {
                return false;
            }
        }
        let held = self.shared.lock().libraries.held.clone();
        let removed = self.update_index(&dir, |list, current| {
            list.retain(|e| e.id != gone);
            if *current == gone {
                current.clone_from(&held);
            }
        });
        if !removed {
            return false;
        }
        // its id may be a new library's later
        self.shared.lock().libraries.read.retain(|(r, _)| *r != gone);
        // the index no longer has it: its files go (the first's stay, for an earlier version), and
        // its pictures and grains once nothing names them
        if gone != FIRST {
            let (sets, choices) = files(&dir, &gone);
            let _ = std::fs::remove_file(sets);
            let _ = std::fs::remove_file(choices);
        }
        true
    }

    /// The library as it ships, added after the others (numbered after its name; shipped brushes
    /// join it only if no other library is the one they join); its place, or none if it couldn't
    /// be kept or the libraries have no folder.
    pub fn restore_stock_brush_library(&self) -> Option<u32> {
        let _writing = library_writes();
        let dir = self.shared.lock().libraries.dir.clone()?;
        let shelf = LibrarySet::shipped(&self.shared.lock().shipped);
        // the round pen to paint, the soft airbrush to erase, the round brush to smudge (as a
        // first start has them)
        let tools = [(1, 0), (3, 0), (2, 0)].map(|origin| crate::brush_library::place_of(&shelf, origin));
        let (list, _) = self.index_now(&dir);
        let id = new_id(&list, &dir);
        let (sets_file, choices_file) = files(&dir, &id);
        if !(self.write_library_file(&sets_file, &shelf)
            && write(&choices_file, &CanvasSession::library_choices(tools)))
        {
            return None;
        }
        let stock = !list.iter().any(|e| e.stock);
        let card = shelf.iter().map(CardSet::of_set).collect();
        let entry = Entry { id: id.clone(), name: unique_in(&list, STOCK_NAME), stock, sets: card };
        self.update_index(&dir, |list, _| list.push(entry)).then(|| self.place_of(&id)).flatten()
    }
}

impl CanvasSession {
    /// The place of library `id` in the libraries as held.
    fn place_of(&self, id: &str) -> Option<u32> {
        self.shared.lock().libraries.place(id).map(|p| p as u32)
    }

    /// The index in `dir` as it is now: as kept (another session may have changed it since), else
    /// as held.
    fn index_now(&self, dir: &Path) -> (Vec<Entry>, String) {
        read_index(dir).unwrap_or_else(|| {
            let st = self.shared.lock();
            (st.libraries.list.clone(), st.libraries.current.clone())
        })
    }

    /// The index in `dir` as it is now (see [`Self::index_now`]) changed by `change` (the list, and
    /// the id of the one in use), kept if that changed it or none is kept, and held as it is then.
    /// False if it couldn't be kept.
    fn update_index(&self, dir: &Path, change: impl FnOnce(&mut Vec<Entry>, &mut String)) -> bool {
        let kept = read_index(dir);
        let (mut list, mut current) = kept.clone().unwrap_or_else(|| {
            let st = self.shared.lock();
            (st.libraries.list.clone(), st.libraries.current.clone())
        });
        change(&mut list, &mut current);
        if list.is_empty() {
            list.push(Entry::first());
        }
        if !list.iter().any(|e| e.id == current) {
            current = list.first().map_or_else(|| FIRST.to_owned(), |e| e.id.clone());
        }
        let written = kept.as_ref() == Some(&(list.clone(), current.clone())) || {
            let json = serde_json::to_string(&IndexFile { libraries: &list, current: &current }).unwrap_or_default();
            write(&dir.join(INDEX_FILE), &json)
        };
        if written {
            let mut st = self.shared.lock();
            st.libraries.list = list;
            st.libraries.current = current;
        }
        written
    }

    /// Library `target` brought into use in place of the one held, which is kept in its files
    /// first (again, if it changes meanwhile), holding [`library_writes`]. False, changing nothing,
    /// if its files can't be read or the one held can't be kept.
    fn bring_into_use(&self, dir: &Path, target: &str) -> bool {
        let (sets_file, choices_file) = files(dir, target);
        // read first: a library that can't be leaves the one in use as it is
        let Some((library, told)) = self.library_at(&sets_file, target != FIRST) else { return false };
        let choices = std::fs::read_to_string(&choices_file).ok();
        let (held_sets, held_choices) = self.files_in_use(dir);
        let mut library = Some(library);
        for _ in 0..KEEP_TRIES {
            // the one held kept as it is left, its card with it
            let Some(kept) = self.save_library_at(&held_sets) else { return false };
            let kept_choices = self.saved_brush_choices();
            if !CanvasSession::write_choices(&held_choices, &kept_choices) {
                return false;
            }
            self.keep_card(dir);
            let mut st = self.shared.lock();
            // changed while it was being kept (a brush chosen meanwhile): kept again
            if !CanvasSession::library_is(&st, kept, &kept_choices) {
                continue;
            }
            let Some(library) = library.take() else { return false };
            st.reset_brush_choices();
            st.put_library(library.clone());
            st.library_kept = Some((sets_file.clone(), told));
            st.libraries.held = target.to_owned();
            // read again once they are wanted: the one left was kept again
            st.libraries.read.clear();
            drop(st);
            if let Some(choices) = choices {
                self.load_brush_choices(choices);
            }
            // the next artwork opens with it, not reading it again
            CanvasSession::remember_library(&sets_file, told, library);
            // (the index not kept, the next start opens the one left, as it was left)
            if !self.update_index(dir, |_, current| *current = target.to_owned()) {
                log::error!("the brush libraries' index couldn't be kept");
            }
            return true;
        }
        log::warn!("the brush library in use kept changing: another wasn't brought into use");
        false
    }

    /// Library `id`'s brushes, by set, read from its files in `dir` (or as read before).
    pub(crate) fn library_brushes(&self, id: &str, dir: &Path) -> Option<Arc<Vec<Vec<Brush>>>> {
        if let Some((_, read)) = self.shared.lock().libraries.read.iter().find(|(r, _)| r == id) {
            return Some(Arc::clone(read));
        }
        let json = std::fs::read_to_string(files(dir, id).0).ok()?;
        let folder = dir.join(crate::brush_pictures::FOLDER);
        let sets = crate::brush_library::read_library(self, &json, Some(&folder), true)?;
        let read = Arc::new(sets.into_iter().map(|set| set.brushes).collect::<Vec<_>>());
        let mut st = self.shared.lock();
        let kept = &mut st.libraries.read;
        kept.retain(|(r, _)| r != id);
        kept.push((id.to_owned(), Arc::clone(&read)));
        if kept.len() > READ_KEPT {
            kept.remove(0);
        }
        Some(read)
    }

    /// The library in use and its choices kept in its own files in `dir`, and its card where the
    /// index keeps one, holding [`library_writes`]; false if they couldn't all be written.
    pub(crate) fn keep_library_in_use(&self, dir: &Path) -> bool {
        let (sets, choices) = self.files_in_use(dir);
        let library = self.save_library_at(&sets).is_some();
        let choices = CanvasSession::write_choices(&choices, &self.saved_brush_choices());
        library && choices && self.keep_card(dir)
    }

    /// The card of the library in use brought up to date in the index in `dir` if it changed (not
    /// while the first library is the only one and no index is kept: a single library's folder
    /// stays as it always was). False if the index couldn't be kept.
    fn keep_card(&self, dir: &Path) -> bool {
        let card: Vec<CardSet> = self.brush_library().iter().map(CardSet::of).collect();
        let (held, single, unchanged) = {
            let st = self.shared.lock();
            let l = &st.libraries;
            let single = matches!(l.list.as_slice(), [only] if *only == Entry::first());
            let unchanged = l.list.iter().find(|e| e.id == l.held).is_some_and(|e| e.sets == card);
            (l.held.clone(), single, unchanged)
        };
        if unchanged || (single && !dir.join(INDEX_FILE).exists()) {
            return true;
        }
        self.update_index(dir, |list, _| {
            if let Some(e) = list.iter_mut().find(|e| e.id == held) {
                e.sets = card;
            }
        })
    }

    /// The files of the library this session holds, in `dir`: its sets, and its choices.
    pub(crate) fn files_in_use(&self, dir: &Path) -> (PathBuf, PathBuf) {
        files(dir, &self.shared.lock().libraries.held)
    }

    /// Every library's sets' file in `dir`, as the index has them.
    pub(crate) fn library_files(&self, dir: &Path) -> Vec<PathBuf> {
        let (list, _) = self.index_now(dir);
        list.iter().map(|e| files(dir, &e.id).0).collect()
    }

    /// The libraries kept in `dir` found (their index), the folder kept for later, and the one in
    /// use put back from its files (else the first, if it can't be): its sets' and choices' files,
    /// and whether it was put back, for [`CanvasSession::open_brushes`].
    pub(crate) fn open_library_in_use(&self, dir: &Path) -> (PathBuf, PathBuf, bool) {
        let (list, current) = libraries_in(dir);
        let first = list.first().map_or_else(|| FIRST.to_owned(), |e| e.id.clone());
        {
            let mut st = self.shared.lock();
            st.libraries = Libraries {
                list,
                current: current.clone(),
                held: current.clone(),
                dir: Some(dir.to_path_buf()),
                read: Vec::new(),
            };
        }
        let (sets, choices) = files(dir, &current);
        if self.load_library_file(&sets, current != FIRST) {
            return (sets, choices, true);
        }
        if current == first {
            return (sets, choices, false);
        }
        // its files gone or unreadable: the first in its place, its own files left as they are
        log::warn!("the brush library in use can't be read: the first one is used");
        self.shared.lock().libraries.held.clone_from(&first);
        let (sets, choices) = files(dir, &first);
        let loaded = self.load_library_file(&sets, first != FIRST);
        (sets, choices, loaded)
    }

    /// A library read from a file (see [`crate::ReadBrushes::Library`]) added after the others
    /// and brought into use: its sets (name, icon, brushes), its tools' brushes (paint, erase,
    /// smudge). Its first set's place, or none if it couldn't be kept or the libraries have no
    /// folder.
    pub(crate) fn add_brush_library(
        &self,
        name: String,
        sets: Vec<(String, u32, Vec<Brush>)>,
        tools: [(usize, usize); 3],
    ) -> Option<u32> {
        let _writing = library_writes();
        let dir = self.shared.lock().libraries.dir.clone()?;
        let mut sets: Vec<LibrarySet> = sets
            .into_iter()
            .map(|(name, icon, brushes)| LibrarySet { origins: vec![None; brushes.len()], name, brushes, icon })
            .collect();
        {
            // their grains at their own size into the grains' files, as brushes brought in have
            let mut st = self.shared.lock();
            st.grain_store.use_folder(&dir);
            for brush in sets.iter_mut().flat_map(|set| set.brushes.iter_mut()) {
                st.grain_store.take(brush);
            }
        }
        let (list, _) = self.index_now(&dir);
        let id = new_id(&list, &dir);
        let (sets_file, choices_file) = files(&dir, &id);
        if !(self.write_library_file(&sets_file, &sets) && write(&choices_file, &CanvasSession::library_choices(tools)))
        {
            return None;
        }
        let name = name.trim();
        let name = unique_in(&list, if name.is_empty() { NEW_NAME } else { name });
        let card = sets.iter().map(CardSet::of_set).collect();
        let entry = Entry { id: id.clone(), name, stock: false, sets: card };
        (self.update_index(&dir, |list, _| list.push(entry)) && self.bring_into_use(&dir, &id)).then_some(0)
    }
}
