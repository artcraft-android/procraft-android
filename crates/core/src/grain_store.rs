//! Brush files' grains at their own size, each kept in a file of its own beside the brush library:
//! brushes hold only which one, so the library and the memory it takes stay as small as they were.
//! Only the last few used are held in memory. A grain whose file can't be read leaves its brush
//! with the smaller image it has always had.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use procraft_engine::brush::{Brush, FullGrain};

/// The folder beside the library the grains' files are in.
const FOLDER: &str = "grains";

/// How many grains are held in memory at once.
const HELD_MOST: usize = 4;

/// How long a grain's file stays, whatever the library says: a brush another window has just
/// brought in may not be in the library kept on disk yet.
const LATELY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Default)]
pub(crate) struct GrainStore {
    folder: Option<PathBuf>,
    /// The last used, the latest last.
    held: Vec<(u64, Arc<Vec<u8>>)>,
    /// Those that came before there was a folder to keep them in.
    unkept: Vec<(u64, Arc<Vec<u8>>)>,
}

impl GrainStore {
    /// Keeps grains in the folder beside the library in `dir` from now on.
    pub(crate) fn use_folder(&mut self, dir: &Path) {
        let folder = dir.join(FOLDER);
        for (id, pixels) in std::mem::take(&mut self.unkept) {
            if !write(&folder, id, &pixels) {
                self.unkept.push((id, pixels));
            }
        }
        self.folder = Some(folder);
    }

    /// Takes `brush`'s grain, as it comes from a file, for keeping.
    pub(crate) fn take(&mut self, brush: &mut Brush) {
        let Some(full) = brush.grain_file.as_mut().and_then(|f| f.full.as_mut()) else { return };
        let Some(pixels) = full.pixels.take() else { return };
        let kept = self.folder.as_deref().is_some_and(|folder| write(folder, full.id, &pixels));
        if !kept && !self.unkept.iter().any(|(id, _)| *id == full.id) {
            self.unkept.push((full.id, pixels));
        }
    }

    /// Takes the files of grains nothing can want any more out of the folder, as a brush that was
    /// deleted leaves its own behind. A file stays if its grain is in `used`, if the text of the
    /// library kept at `library` names it anywhere (a brush in there may not have come back whole),
    /// or if it was kept lately. Files that aren't grains' stay, and all do if the library can't be
    /// read.
    pub(crate) fn forget_all_but(&self, used: &HashSet<u64>, library: &Path) {
        let Some(entries) = self.folder.as_deref().and_then(|folder| fs::read_dir(folder).ok()) else { return };
        let unused: Vec<(u64, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?.strip_suffix(".grain")?;
                let id = u64::from_str_radix(name, 16).ok().filter(|_| name.len() == 16)?;
                let age = entry.metadata().ok()?.modified().ok()?.elapsed().ok()?;
                (age >= LATELY && !used.contains(&id)).then_some((id, path))
            })
            .collect();
        if unused.is_empty() {
            return;
        }
        let Ok(kept) = fs::read_to_string(library) else { return };
        for (id, path) in unused {
            if !kept.contains(&id.to_string())
                && let Err(e) = fs::remove_file(&path)
            {
                log::warn!("a brush's grain file stays: {e}");
            }
        }
    }

    /// `grain`'s pixels; none if it can't be had.
    pub(crate) fn pixels(&mut self, grain: &FullGrain) -> Option<Arc<Vec<u8>>> {
        if let Some(pixels) = &grain.pixels {
            return Some(Arc::clone(pixels));
        }
        if let Some((_, pixels)) = self.unkept.iter().find(|(id, _)| *id == grain.id) {
            return Some(Arc::clone(pixels));
        }
        if let Some(at) = self.held.iter().position(|(id, _)| *id == grain.id) {
            let entry = self.held.remove(at);
            let pixels = Arc::clone(&entry.1);
            self.held.push(entry);
            return Some(pixels);
        }
        let bytes = fs::read(file(self.folder.as_deref()?, grain.id)).ok()?;
        // cut short or not this grain: as good as none
        if bytes.len() != (grain.side as usize).pow(2) {
            log::warn!("a brush's grain file is damaged: using its smaller image");
            return None;
        }
        let pixels = Arc::new(bytes);
        if self.held.len() >= HELD_MOST {
            self.held.remove(0);
        }
        self.held.push((grain.id, Arc::clone(&pixels)));
        Some(pixels)
    }
}

fn file(folder: &Path, id: u64) -> PathBuf {
    folder.join(format!("{id:016x}.grain"))
}

/// Keeps a grain's pixels, whole or not at all (a grain already kept is the same pixels); false if
/// they couldn't be written.
fn write(folder: &Path, id: u64, pixels: &[u8]) -> bool {
    let path = file(folder, id);
    if fs::metadata(&path).is_ok_and(|m| m.len() == pixels.len() as u64) {
        // as lately kept as a new one
        let _ = fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
        return true;
    }
    let written = fs::create_dir_all(folder).map_err(|e| crate::project::file_error("keeping a brush's grain", e));
    let written = written.and_then(|()| {
        crate::project::write_atomically(&path, |p| {
            fs::write(p, pixels).map_err(|e| crate::project::file_error("keeping a brush's grain", e))
        })
    });
    if let Err(e) = &written {
        log::error!("{e}");
    }
    written.is_ok()
}

#[cfg(test)]
mod tests {
    use procraft_engine::brush::{GrainFile, GrainMode, ResetPoint, Source};
    use procraft_engine::brushfile::{self, Pictures};
    use procraft_engine::textures::TEXTURE_SIZE;

    use super::*;
    use crate::CanvasSession;
    use crate::brush_files::sized_gray_png;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-grains-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Makes the file at `path` as if it were kept long ago.
    fn aged(path: &Path) {
        let long_ago = SystemTime::now() - 2 * LATELY;
        fs::File::options().write(true).open(path).unwrap().set_modified(long_ago).unwrap();
    }

    fn brush(fill: u8) -> (Brush, FullGrain) {
        let full = FullGrain::new(512, vec![fill; 512 * 512]).unwrap();
        let file = GrainFile { source: 1, width: 2048, full: Some(full.clone()) };
        (Brush { grain_file: Some(file), ..Brush::round_pen() }, full)
    }

    #[test]
    fn a_grain_goes_into_a_file_and_comes_back_from_it() {
        let dir = folder("back");
        let mut store = GrainStore::default();
        store.use_folder(&dir);
        let (mut b, full) = brush(3);
        store.take(&mut b);
        // the brush holds only which one now
        assert!(b.grain_file.as_ref().and_then(|f| f.full.as_ref()).is_some_and(|g| g.pixels.is_none()));
        let kept = FullGrain { pixels: None, ..full };
        let mut after = GrainStore::default();
        after.use_folder(&dir);
        assert_eq!(after.pixels(&kept).map(|p| p.len()), Some(512 * 512));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grain_from_before_there_was_a_folder_is_kept_when_there_is_one() {
        let dir = folder("late");
        let mut store = GrainStore::default();
        let (mut b, full) = brush(4);
        store.take(&mut b);
        let kept = FullGrain { pixels: None, ..full };
        assert!(store.pixels(&kept).is_some());
        store.use_folder(&dir);
        assert!(store.unkept.is_empty() && file(&dir.join(FOLDER), kept.id).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grain_whose_file_is_gone_or_damaged_leaves_the_brush_its_smaller_image() {
        let dir = folder("gone");
        let mut store = GrainStore::default();
        store.use_folder(&dir);
        let (mut b, full) = brush(5);
        store.take(&mut b);
        let kept = FullGrain { pixels: None, ..full };
        let path = file(&dir.join(FOLDER), kept.id);
        fs::write(&path, [1, 2, 3]).unwrap();
        assert!(store.pixels(&kept).is_none());
        fs::remove_file(&path).unwrap();
        assert!(store.pixels(&kept).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A brush file with a grain 1500 wide at a scale of 0.75.
    fn brush_file() -> Vec<u8> {
        brush_file_with(GrainMode::Texturized)
    }

    fn brush_file_with(mode: GrainMode) -> Vec<u8> {
        let mut b = Brush { name: "Grainy".to_owned(), ..Brush::round_pen() };
        b.grain.mode = mode;
        b.grain.depth = 1.0;
        b.grain.scale = procraft_engine::studio::MAX_GRAIN_SCALE * (0.75f32 / 16.0).sqrt();
        let side = 1500u32;
        let pixels: Vec<u8> = (0..side * side).map(|i| (i % 251) as u8).collect();
        let (shape, grain) = (sized_gray_png(8, 8, &[255; 64]), sized_gray_png(side, side, &pixels));
        let thumbnail = crate::project::rgba_png(1, 1, &[0, 0, 0, 0]);
        let pictures = Pictures {
            shape_png: &shape,
            grain_png: Some(&grain),
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: &thumbnail,
            author_png: None,
            signature_png: None,
            secondary: None,
        };
        brushfile::write(&b, "", &pictures)
    }

    fn imported(s: &CanvasSession) -> Brush {
        let st = s.shared.lock();
        st.library.iter().find_map(|set| set.brushes.iter().find(|b| b.name == "Grainy")).unwrap().clone()
    }

    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("grain_file");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(list) => list.iter_mut().for_each(strip),
            _ => {}
        }
    }

    #[test]
    fn a_library_opens_with_its_grains_or_as_it_was_from_before_they_were_kept_apart() {
        let dir = folder("library");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        s.import_brushes(brush_file()).unwrap();
        let tile = 1500.0 * 0.75 / std::f32::consts::SQRT_2;
        let brought = imported(&s);
        assert!((brought.grain_tile() - tile).abs() < 0.5, "{}", brought.grain_tile());
        let full = brought.grain_file.as_ref().and_then(|f| f.full.clone()).unwrap();
        assert!(full.pixels.is_none() && full.side == 1024);
        let json = s.saved_brush_library();
        assert!(json.len() < 400_000, "the image stayed out of the library: {}", json.len());

        // back as it was left
        assert!(s.load_brush_library(json.clone()));
        let back = imported(&s);
        assert_eq!(back.grain_file_now().and_then(|f| f.full.as_ref()), Some(&full));
        assert!((back.grain_tile() - tile).abs() < 0.5);
        assert!(s.shared.lock().grain_store.pixels(&full).is_some());

        // a library from before: the same brush, its smaller image and the tile it always had
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        strip(&mut old);
        assert!(s.load_brush_library(old.to_string()));
        let before = imported(&s);
        assert!(before.grain_file.is_none() && before.grain.source.is_some());
        assert_eq!(before.grain_tile(), before.grain.scale);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stroke_reads_a_file_s_grain_at_its_own_size_whichever_way_it_moves() {
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return };
        for mode in [GrainMode::Texturized, GrainMode::Moving] {
            let s = CanvasSession::new(64, 64);
            s.use_gpu(gpu.clone());
            let set = s.import_brushes(brush_file_with(mode)).unwrap();
            assert!(s.select_brush(set, 0));
            let mut st = s.shared.lock();
            let layer = st.doc.current();
            let (_, style, _) = st.stroke_brush(crate::ToolKind::Paint, layer);
            assert!(style.full_grain.is_some() && style.grain_side == 1024.0, "{mode:?}");
        }
    }

    #[test]
    fn a_moving_grain_written_out_comes_back_at_the_scale_it_had() {
        let dir = folder("written-moving");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        let set = s.import_brushes(brush_file_with(GrainMode::Moving)).unwrap();
        let scale = imported(&s).grain.file_scale();
        let bytes = s.brush_as_file(set, 0).unwrap();
        let other = CanvasSession::new(64, 64);
        other.import_brushes(bytes).unwrap();
        let back = imported(&other);
        assert!((back.grain.file_scale() - scale).abs() < 1e-3, "{} {scale}", back.grain.file_scale());
        assert_eq!(back.grain_file.and_then(|f| f.full).map(|g| g.side), Some(1024));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_brush_written_out_comes_back_the_same_size() {
        let dir = folder("written");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        let set = s.import_brushes(brush_file()).unwrap();
        let tile = imported(&s).grain_tile();
        let bytes = s.brush_as_file(set, 0).unwrap();
        let other = CanvasSession::new(64, 64);
        other.import_brushes(bytes).unwrap();
        let back = imported(&other);
        assert!((back.grain_tile() - tile).abs() < 1.0, "{} {tile}", back.grain_tile());
        assert_eq!(back.grain_file.and_then(|f| f.full).map(|g| g.side), Some(1024));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_files_of_grains_nothing_can_want_go() {
        let dir = folder("forget");
        let mut store = GrainStore::default();
        store.use_folder(&dir);
        let [used, named, lately, gone] = [6, 7, 8, 9].map(|fill| {
            let (mut b, full) = brush(fill);
            store.take(&mut b);
            full
        });
        let at = |grain: &FullGrain| file(&dir.join(FOLDER), grain.id);
        for grain in [&used, &named, &gone] {
            aged(&at(grain));
        }
        let other = dir.join(FOLDER).join("notes.txt");
        fs::write(&other, "stays").unwrap();
        let library = dir.join("library.json");

        // no library to go by: nothing goes
        store.forget_all_but(&HashSet::from([used.id]), &library);
        assert!(at(&gone).exists());

        // one that names a grain, as a brush that didn't come back would
        fs::write(&library, format!("[{{\"broken\":[{},512]}}]", named.id)).unwrap();
        store.forget_all_but(&HashSet::from([used.id]), &library);
        assert!(at(&used).exists() && at(&named).exists() && at(&lately).exists() && other.exists());
        assert!(!at(&gone).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grain_kept_again_counts_as_kept_lately() {
        let dir = folder("again");
        let mut store = GrainStore::default();
        store.use_folder(&dir);
        let (mut first, full) = brush(6);
        store.take(&mut first);
        let path = file(&dir.join(FOLDER), full.id);
        aged(&path);
        let (mut again, _) = brush(6);
        store.take(&mut again);
        let library = dir.join("library.json");
        fs::write(&library, "[]").unwrap();
        store.forget_all_but(&HashSet::new(), &library);
        assert!(path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grain_stays_while_a_reset_point_or_a_second_holds_it() {
        for by_reset in [true, false] {
            let dir = folder(if by_reset { "held-reset" } else { "held-second" });
            let path = dir.to_string_lossy().into_owned();
            let s = CanvasSession::new(64, 64);
            assert!(s.keep_brushes(path.clone()));
            s.import_brushes(brush_file()).unwrap();
            let id = imported(&s).grain_file.and_then(|f| f.full).unwrap().id;
            // the brush moves on to a grain of its own, only the other still holding the file's
            for set in &mut s.shared.lock().library {
                for b in set.brushes.iter_mut().filter(|b| b.name == "Grainy") {
                    if by_reset {
                        b.reset_point = Some(ResetPoint::of(b, 1));
                    } else {
                        b.secondary = Some(Box::new(b.clone()));
                    }
                    b.grain.source = Source::new(vec![8; TEXTURE_SIZE * TEXTURE_SIZE]);
                }
            }
            assert!(s.keep_brushes(path.clone()));
            let grain = file(&dir.join(FOLDER), id);
            aged(&grain);

            let again = CanvasSession::new(64, 64);
            assert!(again.open_brushes(path));
            assert!(grain.exists(), "{by_reset}");
            let mut st = again.shared.lock();
            assert!(st.grains_held().contains(&id), "{by_reset}");
            let back = st.library.iter().find_map(|set| set.brushes.iter().find(|b| b.name == "Grainy")).unwrap();
            assert!(back.grain_file_now().is_none(), "{by_reset}");
            let full = back.full_grains().first().copied().unwrap().clone();
            assert!(st.grain_store.pixels(&full).is_some(), "{by_reset}");
            drop(st);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_deleted_brush_s_grain_goes_when_the_library_next_opens() {
        let dir = folder("deleted");
        let path = dir.to_string_lossy().into_owned();
        let s = CanvasSession::new(64, 64);
        assert!(s.keep_brushes(path.clone()));
        s.import_brushes(brush_file()).unwrap();
        let id = imported(&s).grain_file.and_then(|f| f.full).unwrap().id;
        assert!(s.keep_brushes(path.clone()));
        let grain = file(&dir.join(FOLDER), id);
        assert!(grain.exists());
        aged(&grain);

        // kept while a brush uses it
        let again = CanvasSession::new(64, 64);
        assert!(again.open_brushes(path.clone()));
        assert!(grain.exists());

        // gone once none does
        for set in &mut again.shared.lock().library {
            set.brushes.retain(|b| b.name != "Grainy");
        }
        assert!(again.keep_brushes(path.clone()));
        let later = CanvasSession::new(64, 64);
        assert!(later.open_brushes(path));
        assert!(!grain.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_last_few_grains_stay_in_memory() {
        let dir = folder("few");
        let mut store = GrainStore::default();
        store.use_folder(&dir);
        for fill in 0..2 * HELD_MOST as u8 {
            let (mut b, full) = brush(fill);
            store.take(&mut b);
            assert!(store.pixels(&FullGrain { pixels: None, ..full }).is_some());
        }
        assert_eq!(store.held.len(), HELD_MOST);
        let _ = fs::remove_dir_all(&dir);
    }
}
