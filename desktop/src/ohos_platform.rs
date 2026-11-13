//! What the app asks of the system through ArkTS (harmony/app's `Host.ets`): the document picker,
//! the share sheet, the pasteboard and the soft keyboard. Requests are JSON, which harmony/bridge
//! hands to ArkTS (a thread-safe call onto the interface's thread); answers come back through
//! [`answer`]. Asking waits for the answer on the app's own thread, never on the interface's thread
//! where ArkTS answers, as rfd's dialogs wait on the desktop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Hands a request (its id, 0 for one wanting no answer, and its JSON) to ArkTS; false if it
/// couldn't.
type Requester = Box<dyn Fn(u32, String) -> bool + Send + Sync>;

static REQUESTER: OnceLock<Requester> = OnceLock::new();
static NEXT: AtomicU32 = AtomicU32::new(1);
/// Requests waiting for their answers, by id.
static WAITING: Mutex<Vec<(u32, mpsc::Sender<Answer>)>> = Mutex::new(Vec::new());
/// The app's cache folder (ArkTS's `context.cacheDir`): files picked are copied into it, and files
/// for the system are written there first.
static CACHE: OnceLock<PathBuf> = OnceLock::new();
/// Files written for the system, handed on once written (see [`hand_on`]).
static OUTGOING: Mutex<Vec<(Instant, Outgoing)>> = Mutex::new(Vec::new());

/// How long a file for the system may take to appear before it is given up on (a write that
/// failed has said so itself).
const OUTGOING_WAIT: Duration = Duration::from_secs(30);

/// ArkTS's answer, its JSON.
pub struct Answer {
    pub value: Value,
}

/// Whether ArkTS can be asked (it registered its side).
pub(crate) fn reachable() -> bool {
    REQUESTER.get().is_some()
}

/// ArkTS is reached through `requester` from now on (harmony/bridge sets it when ArkTS registers).
pub fn set_requester(requester: impl Fn(u32, String) -> bool + Send + Sync + 'static) {
    let _ = REQUESTER.set(Box::new(requester));
}

/// The app's cache folder.
pub(crate) fn set_cache_dir(dir: PathBuf) {
    let _ = CACHE.set(dir);
}

/// ArkTS answered request `id` with `json`.
pub fn answer(id: u32, json: &str) {
    let value = serde_json::from_str(json).unwrap_or(Value::Null);
    let waiting = {
        let mut all = WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        all.iter().position(|(i, _)| *i == id).map(|at| all.swap_remove(at).1)
    };
    if let Some(waiting) = waiting {
        let _ = waiting.send(Answer { value });
    }
}

/// Asks ArkTS `request` and waits for its answer: none if there is no ArkTS to ask or it couldn't
/// answer. Only on the app's thread.
fn ask(request: Value) -> Option<Answer> {
    let requester = REQUESTER.get()?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed).max(1);
    let (sender, receiver) = mpsc::channel();
    WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id, sender));
    if !requester(id, request.to_string()) {
        WAITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|(i, _)| *i != id);
        return None;
    }
    receiver.recv().ok()
}

/// Tells ArkTS `request`, wanting no answer.
fn tell(request: Value) {
    if let Some(requester) = REQUESTER.get() {
        requester(0, request.to_string());
    }
}

fn texts(value: &Value) -> Vec<String> {
    value.as_array().map(|all| all.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// A folder of the cache's, made if need be.
fn cache_folder(name: &str) -> Option<PathBuf> {
    let dir = CACHE.get()?.join(name);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The files the user picks with the document picker (`multiple` or one), of those `extensions`,
/// copied into the app's cache by ArkTS (the app reads paths, the picker gives URIs); none if
/// none was picked.
pub(crate) fn open_files(title: &str, extensions: &[String], multiple: bool) -> Option<Vec<PathBuf>> {
    let into = cache_folder("picked")?;
    let request =
        json!({ "kind": "open", "title": title, "extensions": extensions, "multiple": multiple, "into": into });
    let answer = ask(request)?;
    let paths: Vec<PathBuf> = texts(answer.value.get("paths")?).into_iter().map(PathBuf::from).collect();
    (!paths.is_empty()).then_some(paths)
}

/// Where a file the app is about to write goes: written into the cache first, then shared from
/// the share sheet (a "Share as …" dialog's) or copied where the user picked in the document
/// picker's Save, once written. None if the user picked nowhere.
pub(crate) fn save_file(title: &str, name: &str) -> Option<PathBuf> {
    let name = Path::new(name).file_name().map_or_else(|| "ProCraft".into(), |n| n.to_string_lossy().into_owned());
    let path = cache_folder("out")?.join(&name);
    if title.starts_with("Share") {
        keep_outgoing(Outgoing::Share(vec![path.clone()]));
        return Some(path);
    }
    let answer = ask(json!({ "kind": "save", "title": title, "name": name }))?;
    let uri = answer.value.get("uri")?.as_str().filter(|u| !u.is_empty())?.to_owned();
    keep_outgoing(Outgoing::CopyTo(path.clone(), uri));
    Some(path)
}

/// A folder for files the app is about to write (several, as a "Share as …" of many writes them),
/// shared together from the share sheet once written.
pub(crate) fn folder() -> Option<PathBuf> {
    let dir = cache_folder(&format!("out/{}", NEXT.fetch_add(1, Ordering::Relaxed)))?;
    keep_outgoing(Outgoing::ShareFolder(dir.clone()));
    Some(dir)
}

/// What the app wrote for the system, to hand on once written.
enum Outgoing {
    Share(Vec<PathBuf>),
    ShareFolder(PathBuf),
    /// A file to copy to the URI the document picker's Save gave.
    CopyTo(PathBuf, String),
}

fn keep_outgoing(item: Outgoing) {
    OUTGOING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((Instant::now(), item));
}

/// Hands on what was written for the system once nothing is being written: shared from the share
/// sheet, or copied where the user picked. One whose file isn't there yet waits a while.
pub(crate) fn hand_on() {
    if crate::files::still_writing() {
        return;
    }
    let items = std::mem::take(&mut *OUTGOING.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    let mut waiting = Vec::new();
    for (since, item) in items {
        let ready = match &item {
            Outgoing::Share(paths) => {
                let there: Vec<&PathBuf> = paths.iter().filter(|p| p.is_file()).collect();
                if !there.is_empty() {
                    tell(json!({ "kind": "share", "paths": there }));
                }
                !there.is_empty()
            }
            Outgoing::ShareFolder(dir) => {
                let mut there: Vec<PathBuf> = std::fs::read_dir(dir)
                    .map(|entries| entries.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.is_file()).collect())
                    .unwrap_or_default();
                there.sort();
                if !there.is_empty() {
                    tell(json!({ "kind": "share", "paths": there }));
                }
                !there.is_empty()
            }
            Outgoing::CopyTo(path, uri) => {
                let there = path.is_file();
                if there {
                    tell(json!({ "kind": "copyOut", "from": path, "to": uri }));
                }
                there
            }
        };
        if !ready && since.elapsed() < OUTGOING_WAIT {
            waiting.push((since, item));
        }
    }
    OUTGOING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(waiting);
}

/// Text onto the system's pasteboard.
pub(crate) fn copy_text(text: &str) {
    tell(json!({ "kind": "copyText", "text": text }));
}

/// The system pasteboard's text, if the app may read it (reading takes a permission most apps
/// don't get, or the system's paste button): else none, and the app's own clipboard stands.
pub(crate) fn paste_text() -> Option<String> {
    let answer = ask(json!({ "kind": "pasteText" }))?;
    answer.value.get("text")?.as_str().map(str::to_owned).filter(|t| !t.is_empty())
}

/// The soft keyboard up (a text field has the keys) or down.
pub(crate) fn keyboard(show: bool) {
    tell(json!({ "kind": "keyboard", "show": show }));
}
