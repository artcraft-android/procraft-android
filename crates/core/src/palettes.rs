//! The palettes, the same in every app: kept in a JSON file the app names, each change written at
//! once; with the default one shown under the colour pickers. Colours are 0xRRGGBB.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use procraft_engine::palette::{self, Palette};

/// A palette as the panels show it: its name and its thirty swatches (empty slots none).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PaletteInfo {
    pub name: String,
    pub colors: Vec<Option<u32>>,
}

impl From<&Palette> for PaletteInfo {
    fn from(p: &Palette) -> PaletteInfo {
        PaletteInfo { name: p.name.clone(), colors: p.colors.clone() }
    }
}

/// The palettes and which one is the default.
#[derive(uniffi::Object)]
pub struct Palettes {
    file: Option<PathBuf>,
    kept: Mutex<(Vec<Palette>, usize)>,
    /// Why writing them out failed, not yet asked for, and whether it is failing (told once
    /// until a write works again).
    failure: Mutex<(Option<String>, bool)>,
}

#[uniffi::export]
impl Palettes {
    /// The palettes kept at `path` (or, when there are none there yet, the ones a new install
    /// starts with).
    #[uniffi::constructor]
    pub fn open(path: String) -> Arc<Palettes> {
        let file = PathBuf::from(path);
        let kept = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| palette::read_store(&t))
            .unwrap_or_else(|| (palette::starters(), 0));
        Arc::new(Palettes { file: Some(file), kept: Mutex::new(kept), failure: Mutex::default() })
    }

    /// Why writing the palettes out failed, once until a write works and another fails (for the
    /// app to tell); none if it hasn't.
    pub fn take_failure(&self) -> Option<String> {
        self.failure.lock().unwrap_or_else(|e| e.into_inner()).0.take()
    }

    pub fn list(&self) -> Vec<PaletteInfo> {
        self.kept().0.iter().map(PaletteInfo::from).collect()
    }

    /// The palette shown under the colour pickers.
    pub fn default_palette(&self) -> u32 {
        self.kept().1 as u32
    }

    /// A new empty palette at the end.
    pub fn add(&self) {
        self.change(|p, _| p.push(Palette::empty("Untitled Palette")));
    }

    /// Our example palettes back, those missing added at the end.
    pub fn restore_examples(&self) {
        self.change(|p, _| {
            let missing: Vec<Palette> =
                palette::starters().into_iter().filter(|s| p.iter().all(|q| q.name != s.name)).collect();
            p.extend(missing);
        });
    }

    pub fn rename(&self, index: u32, name: String) {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.change(|p, _| {
            if let Some(q) = p.get_mut(index as usize) {
                q.name = name;
            }
        });
    }

    /// A copy right after it.
    pub fn duplicate(&self, index: u32) {
        self.change(|p, _| {
            let i = index as usize;
            if let Some(q) = p.get(i).cloned() {
                p.insert(i + 1, Palette { name: format!("{} copy", q.name), ..q });
            }
        });
    }

    /// Deletes it (never the last one); the default moves with the list.
    pub fn delete(&self, index: u32) {
        self.change(|p, default| {
            let i = index as usize;
            if p.len() <= 1 || i >= p.len() {
                return;
            }
            p.remove(i);
            *default = match (*default).cmp(&i) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => *default - 1,
                std::cmp::Ordering::Less => *default,
            };
        });
    }

    pub fn choose_default(&self, index: u32) {
        self.change(|p, default| {
            if (index as usize) < p.len() {
                *default = index as usize;
            }
        });
    }

    /// Puts `rgb` in a palette's `slot`, or empties it when none.
    pub fn set_swatch(&self, index: u32, slot: u32, rgb: Option<u32>) {
        self.change(|p, _| {
            if let Some(c) = p.get_mut(index as usize).and_then(|q| q.colors.get_mut(slot as usize)) {
                *c = rgb.map(|v| v & 0xFF_FFFF);
            }
        });
    }

    /// A palette file (ours, or another program's colour swatch or swatch exchange file) added at
    /// the end; false if it isn't one.
    pub fn import_file(&self, bytes: Vec<u8>, file_name: Option<String>) -> bool {
        let read = palette::read_swatches(&bytes).or_else(|| palette::read_other(&bytes, file_name.as_deref()));
        match read {
            Some(p) => {
                self.change(|list, _| list.push(p));
                true
            }
            None => false,
        }
    }

    /// Palette `index` as a swatches file, to share.
    pub fn swatches_file(&self, index: u32) -> Option<Vec<u8>> {
        let kept = self.kept();
        let p = kept.0.get(index as usize)?;
        Some(palette::write_swatches(p, &crate::layers::srgb_profile()))
    }

    /// A palette of the main colours of a picture (`pixels` 0xAARRGGBB), light to dark, added at
    /// the end; false if it has none (all see-through).
    pub fn add_from_pixels(&self, pixels: Vec<u32>) -> bool {
        let opaque: Vec<u32> = pixels.iter().filter(|p| *p >> 24 > 128).map(|p| p & 0xFF_FFFF).collect();
        let colours = palette::main_colours(&opaque, palette::SLOTS);
        if colours.is_empty() {
            return false;
        }
        self.change(|p, _| p.push(Palette::new("Palette from image", colours.into_iter().map(Some))));
        true
    }
}

/// A plain name for a colour (0xRRGGBB), as a palette's cards show it: a grey by its lightness, or
/// a hue with "Dark", "Light" or "Grayish" before it.
#[uniffi::export]
pub fn colour_name(rgb: u32) -> String {
    let [r, g, b] = [16, 8, 0].map(|shift| ((rgb >> shift) & 0xFF) as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.0;
    let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
    if saturation < 0.05 {
        let grey = if lightness > 0.98 {
            "White"
        } else if lightness > 0.75 {
            "Light Gray"
        } else if lightness > 0.35 {
            "Gray"
        } else if lightness > 0.08 {
            "Dark Gray"
        } else {
            "Black"
        };
        return grey.to_owned();
    }
    // the hue in degrees, as a colour picker reads it
    let d = max - min;
    let hue = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    // each name up to the hue it ends at
    const NAMES: [(f32, &str); 18] = [
        (8.0, "Red"),
        (20.0, "Red Orange"),
        (40.0, "Orange"),
        (50.0, "Yellow Orange"),
        (65.0, "Yellow"),
        (80.0, "Yellow Green"),
        (150.0, "Green"),
        (170.0, "Green Cyan"),
        (195.0, "Cyan"),
        (220.0, "Sky Blue"),
        (250.0, "Blue"),
        (275.0, "Violet"),
        (290.0, "Purple"),
        (320.0, "Magenta"),
        (325.0, "Magenta Pink"),
        (342.0, "Pink"),
        (352.0, "Pink Red"),
        (361.0, "Red"),
    ];
    let name = NAMES.iter().find(|(end, _)| hue < *end).map_or("Red", |(_, n)| n);
    if saturation < 0.2 {
        format!("Grayish {name}")
    } else if lightness < 0.5 {
        format!("Dark {name}")
    } else if lightness > 0.8 {
        format!("Light {name}")
    } else {
        name.to_owned()
    }
}

impl Palettes {
    /// Palettes kept nowhere (tests).
    #[cfg(test)]
    pub(crate) fn in_memory() -> Arc<Palettes> {
        Arc::new(Palettes { file: None, kept: Mutex::new((palette::starters(), 0)), failure: Mutex::default() })
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, (Vec<Palette>, usize)> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Changes the palettes and writes them out at once.
    fn change(&self, f: impl FnOnce(&mut Vec<Palette>, &mut usize)) {
        let text = {
            let mut kept = self.kept();
            let (list, default) = &mut *kept;
            f(list, default);
            *default = (*default).min(list.len().saturating_sub(1));
            palette::write_store(list, *default)
        };
        if let Some(file) = &self.file {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = file.with_extension("json.tmp");
            let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file));
            let mut failure = self.failure.lock().unwrap_or_else(|e| e.into_inner());
            match written {
                Ok(()) => failure.1 = false,
                Err(e) => {
                    log::error!("saving the palettes: {e}");
                    if !failure.1 {
                        *failure = (Some(e.to_string()), true);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_that_cant_be_written_say_so_once() {
        // a file where their folder would be
        let blocked = std::env::temp_dir().join(format!("procraft-palettes-blocked-{}", std::process::id()));
        std::fs::write(&blocked, b"not a folder").unwrap();
        let p = Palettes::open(blocked.join("palettes.json").to_string_lossy().into_owned());
        assert_eq!(p.take_failure(), None);
        p.add();
        let told = p.take_failure();
        p.add();
        let again = p.take_failure();
        let _ = std::fs::remove_file(&blocked);
        assert!(told.is_some(), "a failed write told");
        assert_eq!(again, None, "not told again while failing");
        assert_eq!(p.list().len(), 4, "the palettes as changed, though not kept");
    }

    #[test]
    fn palettes_change_and_keep_their_default() {
        let p = Palettes::in_memory();
        assert_eq!(p.list().len(), 2);
        p.add();
        p.choose_default(2);
        p.set_swatch(2, 0, Some(0x123456));
        p.rename(2, "  Mine ".into());
        assert_eq!(
            p.list().get(2).map(|q| (q.name.clone(), q.colors.first().copied().flatten())),
            Some(("Mine".into(), Some(0x123456)))
        );
        p.delete(0);
        assert_eq!(p.default_palette(), 1);
        p.delete(1);
        assert_eq!(p.default_palette(), 0);
        p.delete(0);
        assert_eq!(p.list().len(), 1, "the last palette stays");
        p.restore_examples();
        assert_eq!(p.list().len(), 2);
        let file = p.swatches_file(0).unwrap();
        assert!(p.import_file(file, Some("Basics.swatches".into())));
        assert!(!p.import_file(vec![1, 2, 3], None));
        assert!(p.add_from_pixels(vec![0xFF102030, 0x00FFFFFF, 0xFFF0E0D0]));
        assert_eq!(p.list().last().map(|q| q.colors[..3].to_vec()), Some(vec![Some(0xF0E0D0), Some(0x102030), None]));
    }

    #[test]
    fn colours_get_plain_names() {
        assert_eq!(colour_name(0xFFFFFF), "White");
        assert_eq!(colour_name(0x808080), "Gray");
        assert_eq!(colour_name(0x000000), "Black");
        assert_eq!(colour_name(0xFF0000), "Red");
        assert_eq!(colour_name(0x800000), "Dark Red");
        assert_eq!(colour_name(0x0000FF), "Blue");
        assert_eq!(colour_name(0xA0C8FF), "Light Sky Blue");
        assert_eq!(colour_name(0x8C7A7A), "Grayish Red");
    }
}
