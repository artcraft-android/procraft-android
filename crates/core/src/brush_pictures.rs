//! The pictures of the library's brushes (their shapes, grains, previews and makers' pictures),
//! each kept once, in a file of its own in a folder beside the library, named by what tells it
//! apart. The library names them, so it stays small however many brushes are brought in: keeping
//! it after a change writes only the pictures it hasn't kept yet, not every brush's again.
//! Libraries from before, with the pictures inside them, are read as they are.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// The folder beside the library the pictures are kept in.
pub(crate) const FOLDER: &str = "pictures";

/// How the library names a picture in a file, before its name (base64, how pictures were kept
/// inside it before, has no ':').
pub(crate) const MARK: &str = "picture:";

/// How long a picture's file stays, whatever the library says: a brush another window has just
/// brought in may not be in the library kept on disk yet.
const LATELY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A picture kept again whose file is older than this is marked as kept now, so that it is never
/// taken for one nothing has wanted lately (once a day at most, not at every save).
const TOUCHED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// A picture to keep: a file's bytes (a PNG), or a shape's or grain's pixels.
#[derive(Clone)]
pub(crate) enum Held {
    Bytes(Arc<[u8]>),
    Pixels(Arc<Vec<u8>>),
}

impl Held {
    fn bytes(&self) -> &[u8] {
        match self {
            Held::Bytes(b) => b,
            Held::Pixels(p) => p,
        }
    }
}

/// The name a picture of `bytes` is kept by.
pub(crate) fn bytes_name(bytes: &[u8]) -> String {
    format!("{:016x}.picture", crate::project::pixels_hash(bytes))
}

/// The name a shape's or grain's pixels, known by `id`, are kept by.
pub(crate) fn pixels_name(id: u64) -> String {
    format!("{id:016x}.pixels")
}

/// Whether `name` is one a picture is kept by: 16 hex digits and its kind, so nothing outside the
/// folder can be named.
fn is_name(name: &str) -> bool {
    let Some((id, kind)) = name.split_once('.') else { return false };
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()) && matches!(kind, "picture" | "pixels")
}

/// Keeps `pictures` in `folder`, each whole or not at all; one already there is left as it is,
/// marked as kept lately if it is old. False if one couldn't be kept. (Not flushed to the disk:
/// that cost a quarter of a second more for 176 new pictures here, and saves only against a power
/// cut in the seconds after a brush is brought in.)
pub(crate) fn keep(folder: &Path, pictures: &[(String, Held)]) -> bool {
    let mut all = true;
    let mut made = false;
    let mut seen = HashSet::new();
    let what = "keeping a brush's picture";
    for (name, held) in pictures {
        // (a reset point or a second brush names its pictures again)
        if !is_name(name) || !seen.insert(name.as_str()) {
            continue;
        }
        let path = folder.join(name);
        let bytes = held.bytes();
        if let Ok(meta) = fs::metadata(&path)
            && meta.len() == bytes.len() as u64
        {
            let old = meta.modified().ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age >= TOUCHED_AFTER);
            if old {
                let _ = fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
            }
            continue;
        }
        let mut written = Ok(());
        if !made {
            written = fs::create_dir_all(folder).map_err(|e| crate::project::file_error(what, e));
            made = written.is_ok();
        }
        let written = written.and_then(|()| {
            crate::project::write_atomically(&path, |p| {
                fs::write(p, bytes).map_err(|e| crate::project::file_error(what, e))
            })
        });
        if let Err(e) = written {
            log::error!("{e}");
            all = false;
        }
    }
    all
}

/// The pictures the library text `json` names, read from `folder` (one that isn't there or can't
/// be read is left out: its brush keeps what it would without it).
pub(crate) fn read(folder: &Path, json: &str) -> HashMap<String, Arc<Vec<u8>>> {
    let mut out = HashMap::new();
    for (at, _) in json.match_indices(MARK) {
        let rest = &json[at + MARK.len()..];
        let name = &rest[..rest.find('"').unwrap_or(rest.len())];
        if !is_name(name) || out.contains_key(name) {
            continue;
        }
        match fs::read(folder.join(name)) {
            Ok(bytes) => {
                out.insert(name.to_owned(), Arc::new(bytes));
            }
            Err(e) => log::warn!("a brush's picture can't be read: {e}"),
        }
    }
    out
}

/// The bytes a kept brush's `text` gives: a picture's in `pictures` (as read by [`read`]), or
/// those the text holds itself (base64), as the library kept them before.
pub(crate) fn bytes_of(text: &str, pictures: &HashMap<String, Arc<Vec<u8>>>) -> Option<Vec<u8>> {
    match text.strip_prefix(MARK) {
        Some(name) => pictures.get(name).map(|bytes| bytes.to_vec()),
        None => procraft_engine::base64::decode(text),
    }
}

/// The names of pictures the library text `json` names.
fn named(json: &str) -> HashSet<&str> {
    json.match_indices(MARK)
        .map(|(at, _)| {
            let rest = &json[at + MARK.len()..];
            &rest[..rest.find('"').unwrap_or(rest.len())]
        })
        .collect()
}

/// Whether `name` is a write of a picture cut short (`<16 hex>.<number>.tmp`), as a crash leaves.
fn is_leftover(name: &str) -> bool {
    let mut parts = name.split('.');
    let (Some(id), Some(n), Some("tmp"), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    id.len() == 16
        && id.bytes().all(|b| b.is_ascii_hexdigit())
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
}

/// The shape or grain a kept brush's `text` gives: the pixels read from a picture's file (see
/// [`read`]), shared with every other brush naming it, or those the text holds itself.
pub(crate) fn source_of(
    text: &str,
    pictures: &HashMap<String, Arc<Vec<u8>>>,
) -> Option<procraft_engine::brush::Source> {
    match text.strip_prefix(MARK) {
        Some(name) => pictures.get(name).cloned().and_then(procraft_engine::brush::Source::shared),
        None => procraft_engine::brush::Source::from_text(text),
    }
}

/// Takes the files of pictures the library kept at `library` no longer names out of `folder`, as
/// a brush that was deleted leaves its own behind, and writes of them a crash cut short; one kept
/// lately stays, and all do if the library can't be read. Files that aren't pictures' stay. Call it
/// holding the library's own lock, so that a library being kept meanwhile can't name one it takes.
pub(crate) fn forget_unnamed(folder: &Path, library: &Path) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    let old: Vec<(String, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok().filter(|n| is_name(n) || is_leftover(n))?;
            let age = entry.metadata().ok()?.modified().ok()?.elapsed().ok()?;
            (age >= LATELY).then(|| (name, entry.path()))
        })
        .collect();
    if old.is_empty() {
        return;
    }
    let Ok(kept) = fs::read_to_string(library) else { return };
    let named = named(&kept);
    for (name, path) in old {
        if !named.contains(name.as_str())
            && let Err(e) = fs::remove_file(&path)
        {
            log::warn!("a brush's picture file stays: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-pictures-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_picture_is_kept_once_and_read_back_by_its_name() {
        let dir = folder("once");
        let png: Arc<[u8]> = vec![1, 2, 3, 4].into();
        let name = bytes_name(&png);
        assert!(keep(&dir, &[(name.clone(), Held::Bytes(Arc::clone(&png)))]));
        let json = format!("{{\"thumbnail\":\"{MARK}{name}\",\"grain\":\"{MARK}0000000000000000.pixels\"}}");
        let read = read(&dir, &json);
        // the one there, read; the one not there, left out
        assert_eq!(read.len(), 1);
        assert_eq!(bytes_of(&format!("{MARK}{name}"), &read), Some(vec![1, 2, 3, 4]));
        assert_eq!(bytes_of(&format!("{MARK}0000000000000000.pixels"), &read), None);
        // the way the library kept them before: inside it
        assert_eq!(bytes_of(&procraft_engine::base64::encode(&[9, 8]), &read), Some(vec![9, 8]));
        // a shape from its file: the same pixels for every brush naming it, not a copy each
        let side = procraft_engine::textures::TEXTURE_SIZE;
        let pixels: Arc<Vec<u8>> = Arc::new((0..side * side).map(|i| (i % 7) as u8).collect());
        let shapes = HashMap::from([(pixels_name(1), Arc::clone(&pixels))]);
        let text = format!("{MARK}{}", pixels_name(1));
        let (a, b) = (source_of(&text, &shapes).unwrap(), source_of(&text, &shapes).unwrap());
        assert!(Arc::ptr_eq(&a.pixels, &b.pixels) && Arc::ptr_eq(&a.pixels, &pixels));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_picture_kept_again_is_marked_lately_and_leftover_writes_go() {
        let dir = folder("again");
        let png: Arc<[u8]> = vec![5, 6, 7].into();
        let name = bytes_name(&png);
        let pictures = [(name.clone(), Held::Bytes(Arc::clone(&png))), (name.clone(), Held::Bytes(Arc::clone(&png)))];
        assert!(keep(&dir, &pictures));
        let long_ago = SystemTime::now() - 2 * LATELY;
        let age = |p: &Path| fs::metadata(p).unwrap().modified().unwrap().elapsed().unwrap_or_default();
        fs::File::options().write(true).open(dir.join(&name)).unwrap().set_modified(long_ago).unwrap();
        // kept again: no longer old, so nothing takes it while a library naming it is written
        assert!(keep(&dir, &pictures));
        assert!(age(&dir.join(&name)) < TOUCHED_AFTER);
        // a write a crash cut short, old: goes with the next cleanup; the picture named stays
        let leftover = dir.join("0123456789abcdef.3.tmp");
        fs::write(&leftover, [1]).unwrap();
        fs::File::options().write(true).open(&leftover).unwrap().set_modified(long_ago).unwrap();
        fs::File::options().write(true).open(dir.join(&name)).unwrap().set_modified(long_ago).unwrap();
        let library = dir.join("library.json");
        fs::write(&library, format!("[{{\"thumbnail\":\"{MARK}{name}\"}}]")).unwrap();
        forget_unnamed(&dir, &library);
        assert!(!leftover.exists() && dir.join(&name).exists());
        assert!(!is_leftover("0123456789abcdef.picture") && !is_leftover("../x.1.tmp"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_reaches_nothing_outside_the_folder() {
        for name in ["../library.json", "0123456789abcdef.picture/..", "0123456789abcdeg.picture", "x.pixels"] {
            assert!(!is_name(name), "{name}");
        }
        assert!(is_name("0123456789abcdef.pixels"));
        let dir = folder("outside");
        assert!(keep(&dir, &[("../outside.picture".to_owned(), Held::Bytes(vec![1].into()))]));
        assert!(!dir.with_file_name("outside.picture").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
