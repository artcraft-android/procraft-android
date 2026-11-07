//! Grains and shapes at their own size, from brush files or the user's own pictures, each kept in
//! a file of its own beside the brush library:
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

    /// Takes `brush`'s grain and shape, and its second's, as they come from a file, for keeping.
    pub(crate) fn take(&mut self, brush: &mut Brush) {
        let grain = brush.grain_file.as_mut().and_then(|f| f.full.as_mut());
        let shape = brush.shape_file.as_mut().map(|f| &mut f.full);
        for full in grain.into_iter().chain(shape) {
            self.keep(full);
        }
        if let Some(second) = brush.secondary.as_deref_mut() {
            self.take(second);
        }
    }

    /// Takes `full`'s pixels for keeping, if it still holds them.
    pub(crate) fn keep(&mut self, full: &mut FullGrain) {
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
    use procraft_engine::brush::{FULL_GRAIN_MOST, GrainFile, GrainMode, ResetPoint, Source};
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

    /// A dual brush file with a shape 1500 wide, its second's another of the same size.
    fn shaped_file() -> Vec<u8> {
        let side = 1500u32;
        let image = |seed: u32| {
            sized_gray_png(
                side,
                side,
                &(0..side * side).map(|i| (i.wrapping_add(seed) % 251) as u8).collect::<Vec<u8>>(),
            )
        };
        let (shape, other) = (image(0), image(7));
        let thumbnail = crate::project::rgba_png(1, 1, &[0, 0, 0, 0]);
        let pictures_of = |shape_png, secondary| Pictures {
            shape_png,
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: &thumbnail,
            author_png: None,
            signature_png: None,
            secondary,
        };
        let pictures = pictures_of(&shape, Some(Box::new(pictures_of(&other, None))));
        let b = Brush {
            name: "Shaped".to_owned(),
            secondary: Some(Box::new(Brush { name: "Second".to_owned(), ..Brush::round_pen() })),
            ..Brush::round_pen()
        };
        brushfile::write(&b, "", &pictures)
    }

    fn shaped(s: &CanvasSession) -> Brush {
        let st = s.shared.lock();
        let set = st.library.iter().find(|set| set.brushes.iter().any(|b| b.name == "Shaped"));
        set.and_then(|set| set.brushes.iter().find(|b| b.name == "Shaped")).cloned().unwrap()
    }

    fn without_shape_full(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("shape_full");
                map.values_mut().for_each(without_shape_full);
            }
            serde_json::Value::Array(list) => list.iter_mut().for_each(without_shape_full),
            _ => {}
        }
    }

    #[test]
    fn a_library_opens_with_its_shapes_or_as_it_was_from_before_they_were_kept_apart() {
        let dir = folder("shapes");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        s.import_brushes(shaped_file()).unwrap();
        let brought = shaped(&s);
        let full = brought.shape_file_now().map(|f| f.full.clone()).unwrap();
        assert!(full.pixels.is_none() && full.side == 1024);
        let second = brought.secondary.as_ref().and_then(|b| b.shape_file_now()).map(|f| f.full.clone()).unwrap();
        assert!(second.pixels.is_none() && second.id != full.id, "taken for keeping, the second's too");
        assert_eq!(brought.full_grains().len(), 2);
        let json = s.saved_brush_library();
        assert!(json.len() < 800_000, "the images stayed out of the library: {}", json.len());

        // back as it was left
        assert!(s.load_brush_library(json.clone()));
        let back = shaped(&s);
        assert_eq!(back.shape_file_now().map(|f| &f.full), Some(&full));
        assert!(s.shared.lock().grain_store.pixels(&full).is_some());
        assert!(s.shared.lock().grain_store.pixels(&second).is_some());

        // a library from before: the same brush, its smaller image
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        without_shape_full(&mut old);
        assert!(s.load_brush_library(old.to_string()));
        let before = shaped(&s);
        assert!(before.shape_file.is_none() && before.shape.source.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stroke_and_its_second_read_a_file_s_shape_at_its_own_size() {
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return };
        let s = CanvasSession::new(64, 64);
        s.use_gpu(gpu.clone());
        let set = s.import_brushes(shaped_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Shaped").unwrap();
        assert!(s.select_brush(set, index as u32));
        let mut st = s.shared.lock();
        let layer = st.doc.current();
        let (brush, style, _) = st.stroke_brush(crate::ToolKind::Paint, layer);
        assert!(style.full_shape.is_some() && style.shape == Some(0));
        let (_, second, _) = st.second_brush(crate::ToolKind::Paint, layer, &brush, &style).unwrap();
        assert!(second.style.full_shape.is_some() && second.style.full_shape != style.full_shape);
    }

    #[test]
    fn the_studio_shows_a_file_s_shape_at_its_own_size_and_edits_it_as_ever() {
        let s = CanvasSession::new(64, 64);
        let set = s.import_brushes(shaped_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Shaped").unwrap();
        assert!(s.select_brush(set, index as u32));
        s.studio_begin();
        let side = procraft_engine::textures::TEXTURE_SIZE;
        assert_eq!(s.studio_source_picture(crate::studio::SourceKind::Shape).len(), 1024 * 1024);
        assert_eq!(s.studio_source(crate::studio::SourceKind::Shape).len(), side * side);
        // what isn't a file's shape goes on as it was
        assert_eq!(
            s.studio_source_picture(crate::studio::SourceKind::Grain),
            s.studio_source(crate::studio::SourceKind::Grain)
        );
        // a shape of the user's own leaves the file's behind
        assert!(s.set_studio_source(crate::studio::SourceKind::Shape, vec![9; side * side]));
        assert_eq!(s.studio_source_picture(crate::studio::SourceKind::Shape).len(), side * side);
        s.studio_end(false);
    }

    #[test]
    fn a_shape_changed_leaves_the_file_s_image_behind() {
        let s = CanvasSession::new(64, 64);
        s.import_brushes(shaped_file()).unwrap();
        let mut b = shaped(&s);
        assert!(b.shape_file_now().is_some());
        b.shape.source = procraft_engine::brush::Source::new(vec![9; procraft_engine::textures::TEXTURE_SIZE.pow(2)]);
        assert!(b.shape_file_now().is_none());
    }

    /// A checkerboard `n` square: detail no smaller image can hold.
    fn fine(n: usize, seed: usize) -> Vec<u8> {
        (0..n * n).map(|i| if (i % n + i / n + seed).is_multiple_of(2) { 255 } else { 0 }).collect()
    }

    fn studio_brush(s: &CanvasSession) -> Brush {
        let st = s.shared.lock();
        st.brush(st.tool).clone()
    }

    #[test]
    fn a_picture_is_kept_at_its_own_size_up_to_the_most() {
        let side =
            |w: u32, h: u32| crate::studio::source_from_picture(vec![200; (w * h * 4) as usize], w, h).len().isqrt();
        assert_eq!(side(100, 40), TEXTURE_SIZE);
        assert_eq!(side(256, 256), TEXTURE_SIZE);
        assert_eq!(side(300, 100), 512);
        assert_eq!(side(100, 700), 1024);
        assert_eq!(side(2000, 1000), FULL_GRAIN_MOST);
        assert!(crate::studio::source_from_picture(vec![1; 7], 2, 2).is_empty());
    }

    #[test]
    fn a_wide_picture_sits_in_the_middle_of_its_square_at_any_size() {
        let (w, h) = (1000u32, 500u32);
        let out = crate::studio::source_from_picture(vec![255; (w * h * 4) as usize], w, h);
        let n = out.len().isqrt();
        assert_eq!(n, 1024);
        let row = |y: usize| out[y * n + n / 2];
        assert_eq!((row(n / 8), row(n / 2), row(n - n / 8)), (0, 255, 0));
        // half the square's height, as the picture is half as high as it is wide
        let lit = (0..n).filter(|&y| row(y) > 128).count();
        assert!(lit.abs_diff(n / 2) <= 2, "{lit}");
    }

    #[test]
    fn a_big_source_comes_down_to_what_a_brush_always_holds() {
        let n = TEXTURE_SIZE;
        assert_eq!(crate::studio::small_source(vec![7; 1024 * 1024]), vec![7; n * n]);
        assert_eq!(crate::studio::small_source(vec![7; n * n]), vec![7; n * n]);
        assert_eq!(crate::studio::small_source(vec![7; 5]), vec![7; 5]);
        assert!(crate::studio::small_source(Vec::new()).is_empty());
    }

    #[test]
    fn the_user_s_own_big_shape_and_grain_are_kept_as_a_file_s_are() {
        use crate::studio::SourceKind::{Grain, Shape};
        let dir = folder("own");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        let (shape, grain) = (fine(1024, 0), fine(512, 1));
        s.studio_begin();
        assert!(s.set_studio_source(Shape, shape.clone()));
        assert!(s.set_studio_source(Grain, grain.clone()));
        // all of it to show and edit, and what a brush always holds beside it
        assert_eq!(s.studio_source_picture(Shape), shape);
        assert_eq!(s.studio_source_picture(Grain), grain);
        assert_eq!(s.studio_source(Shape).len(), TEXTURE_SIZE * TEXTURE_SIZE);
        assert_eq!(s.studio_source(Grain).len(), TEXTURE_SIZE * TEXTURE_SIZE);
        s.studio_end(true);

        let b = studio_brush(&s);
        let (shape_full, grain_full) =
            (b.shape_file_now().unwrap().full.clone(), b.grain_file_now().unwrap().full.clone().unwrap());
        assert!(shape_full.pixels.is_none() && grain_full.pixels.is_none(), "taken for keeping");
        assert!(file(&dir.join(FOLDER), shape_full.id).exists() && file(&dir.join(FOLDER), grain_full.id).exists());

        // back as it was left
        let json = s.saved_brush_library();
        assert!(s.load_brush_library(json));
        let back = studio_brush(&s);
        assert_eq!(back.shape_file_now().map(|f| &f.full), Some(&shape_full));
        assert_eq!(back.grain_file_now().and_then(|f| f.full.as_ref()), Some(&grain_full));
        assert_eq!(s.shared.lock().grain_store.pixels(&shape_full).unwrap().as_slice(), shape.as_slice());
        assert_eq!(s.shared.lock().grain_store.pixels(&grain_full).unwrap().as_slice(), grain.as_slice());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stroke_reads_the_user_s_own_big_shape_and_grain_at_their_own_size() {
        use crate::studio::SourceKind::{Grain, Shape};
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return };
        let s = CanvasSession::new(64, 64);
        s.use_gpu(gpu.clone());
        s.studio_begin();
        assert!(s.set_studio_source(Shape, fine(1024, 0)));
        assert!(s.set_studio_source(Grain, fine(1024, 1)));
        s.studio_end(true);
        let mut st = s.shared.lock();
        let layer = st.doc.current();
        let (_, style, _) = st.stroke_brush(crate::ToolKind::Paint, layer);
        assert!(style.full_shape.is_some() && style.shape == Some(0));
        assert!(style.full_grain.is_some());
    }

    #[test]
    fn a_small_picture_stays_as_it_always_was() {
        use crate::studio::SourceKind::{Grain, Shape};
        let s = CanvasSession::new(64, 64);
        s.studio_begin();
        assert!(s.set_studio_source(Shape, fine(TEXTURE_SIZE, 0)));
        assert!(s.set_studio_source(Grain, fine(TEXTURE_SIZE, 1)));
        s.studio_end(true);
        let b = studio_brush(&s);
        assert!(b.shape_file_now().is_none() && b.grain_file_now().is_none());
    }

    #[test]
    fn a_material_takes_a_big_picture_at_the_size_it_always_had() {
        use crate::studio::SourceKind::Metallic;
        let s = CanvasSession::new(64, 64);
        s.studio_begin();
        assert!(s.set_studio_source(Metallic, vec![90; 1024 * 1024]));
        assert_eq!(s.studio_source_picture(Metallic), vec![90; TEXTURE_SIZE * TEXTURE_SIZE]);
        s.studio_end(false);
    }

    #[test]
    fn only_a_square_a_brush_can_keep_is_taken() {
        use crate::studio::SourceKind::Shape;
        let s = CanvasSession::new(64, 64);
        s.studio_begin();
        for side in [128, 300, 2048] {
            assert!(!s.set_studio_source(Shape, vec![9; side * side]), "{side}");
        }
        assert!(!s.set_studio_source(Shape, vec![9; 5]));
        s.studio_end(false);
    }

    #[test]
    fn a_tick_with_nothing_changed_leaves_a_file_s_images_alone() {
        use crate::studio::SourceKind::{Grain, Shape};
        let s = CanvasSession::new(64, 64);
        let set = s.import_brushes(brush_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Grainy").unwrap();
        assert!(s.select_brush(set, index as u32));
        let before = imported(&s);
        s.studio_begin();
        assert!(!s.set_studio_source(Grain, s.studio_source_picture(Grain)));
        assert!(!s.set_studio_source(Shape, s.studio_source_picture(Shape)));
        s.studio_end(true);
        let after = imported(&s);
        assert_eq!(after.grain_file_now(), before.grain_file_now());
        assert_eq!(after.grain_tile(), before.grain_tile());
    }

    #[test]
    fn a_grain_changed_from_a_file_s_leaves_its_width_behind() {
        use crate::studio::SourceKind::Grain;
        let s = CanvasSession::new(64, 64);
        let set = s.import_brushes(brush_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Grainy").unwrap();
        assert!(s.select_brush(set, index as u32));
        s.studio_begin();
        assert!(s.set_studio_source(Grain, fine(1024, 1)));
        s.studio_end(true);
        let b = imported(&s);
        assert_eq!(b.grain_file_now().map(|f| f.width), Some(0));
        assert_eq!(b.grain_tile(), b.grain.scale);
    }

    #[test]
    fn the_user_s_own_big_shape_written_out_comes_back_at_its_own_size() {
        use crate::studio::SourceKind::Shape;
        let dir = folder("own-written");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        let set = s.import_brushes(brush_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Grainy").unwrap();
        assert!(s.select_brush(set, index as u32));
        s.studio_begin();
        assert!(s.set_studio_source(Shape, fine(1024, 0)));
        s.studio_end(true);
        let id = imported(&s).shape_file_now().map(|f| f.full.id).unwrap();
        let bytes = s.brush_as_file(set, index as u32).unwrap();
        let other = CanvasSession::new(64, 64);
        other.import_brushes(bytes).unwrap();
        assert_eq!(imported(&other).shape_file_now().map(|f| (f.full.side, f.full.id)), Some((1024, id)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shape_written_out_comes_back_at_its_own_size() {
        let dir = folder("written-shape");
        let s = CanvasSession::new(64, 64);
        s.save_brush_library_file(dir.join("library.json").to_string_lossy().into_owned());
        let set = s.import_brushes(shaped_file()).unwrap();
        let index = s.brush_library()[set as usize].brushes.iter().position(|b| b.name == "Shaped").unwrap();
        let id = shaped(&s).shape_file_now().map(|f| f.full.id).unwrap();
        let bytes = s.brush_as_file(set, index as u32).unwrap();
        let other = CanvasSession::new(64, 64);
        other.import_brushes(bytes).unwrap();
        let back = shaped(&other);
        // the same pixels: 1024 written, 1024 read, none lost
        assert_eq!(back.shape_file_now().map(|f| (f.full.side, f.full.id)), Some((1024, id)));
        let _ = fs::remove_dir_all(&dir);
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
