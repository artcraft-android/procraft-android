//! Gradient Map's gradient library, the same in every app: kept in a JSON file the app names
//! (`{"gradients": [{"name", "stops": [[position, 0xAARRGGBB], ...]}]}`), each change written at
//! once; with nothing kept yet it holds ours.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A gradient's colour stop: where along it (0..1) and its colour, 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct GradientStop {
    pub position: f32,
    pub rgb: u32,
}

/// A gradient to map by: its name and colour stops, in any order (two at least).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GradientInfo {
    pub name: String,
    pub stops: Vec<GradientStop>,
}

/// ProCraft's own gradients.
fn ours() -> Vec<GradientInfo> {
    let g = |name: &str, stops: &[(f32, u32)]| GradientInfo {
        name: name.to_owned(),
        stops: stops.iter().map(|&(position, rgb)| GradientStop { position, rgb }).collect(),
    };
    vec![
        g("Ember", &[(0.0, 0x140A1E), (0.45, 0x9C2A1F), (0.8, 0xF29B38), (1.0, 0xFFF4D6)]),
        g("Glacier", &[(0.0, 0x0B1A33), (0.5, 0x3F7FB5), (1.0, 0xE8F6FF)]),
        g("Moss", &[(0.0, 0x101A0C), (0.5, 0x5C7A34), (1.0, 0xE9F0C8)]),
        g("Citrus", &[(0.0, 0x2B1640), (0.5, 0xE0466A), (1.0, 0xFFE15C)]),
        g("Graphite", &[(0.0, 0x000000), (1.0, 0xFFFFFF)]),
        g("Lagoon", &[(0.0, 0x06202A), (0.55, 0x1B9A8E), (1.0, 0xF4F1D0)]),
    ]
}

/// The colour gradient `g` has at `p` (0..1): its stops' colours mixed between them.
pub fn colour_at(g: &GradientInfo, p: f32) -> u32 {
    let mut stops = g.stops.clone();
    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
    let Some(first) = stops.first() else { return 0 };
    if p <= first.position {
        return first.rgb;
    }
    for pair in stops.windows(2) {
        let &[a, b] = pair else { continue };
        if p <= b.position {
            let t = if b.position > a.position { (p - a.position) / (b.position - a.position) } else { 1.0 };
            let mix = |shift: u32| {
                let (ca, cb) = (((a.rgb >> shift) & 0xFF) as f32, ((b.rgb >> shift) & 0xFF) as f32);
                ((ca + (cb - ca) * t) as i32).clamp(0, 255) as u32
            };
            return (mix(16) << 16) | (mix(8) << 8) | mix(0);
        }
    }
    stops.last().map_or(0, |s| s.rgb)
}

fn read(text: &str) -> Option<Vec<GradientInfo>> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let list: Vec<GradientInfo> = root
        .get("gradients")?
        .as_array()?
        .iter()
        .filter_map(|g| {
            let name = g.get("name")?.as_str()?.to_owned();
            let stops = g
                .get("stops")?
                .as_array()?
                .iter()
                .filter_map(|s| {
                    let position = s.get(0)?.as_f64()? as f32;
                    let colour = s.get(1)?.as_i64()?;
                    Some(GradientStop { position: position.clamp(0.0, 1.0), rgb: (colour & 0xFF_FFFF) as u32 })
                })
                .collect::<Vec<_>>();
            (stops.len() >= 2).then_some(GradientInfo { name, stops })
        })
        .collect();
    (!list.is_empty()).then_some(list)
}

fn write(list: &[GradientInfo]) -> String {
    let gradients: Vec<serde_json::Value> = list
        .iter()
        .map(|g| {
            let stops: Vec<serde_json::Value> =
                g.stops.iter().map(|s| serde_json::json!([s.position, i64::from(s.rgb) | 0xFF00_0000])).collect();
            serde_json::json!({ "name": g.name, "stops": stops })
        })
        .collect();
    serde_json::json!({ "gradients": gradients }).to_string()
}

/// The gradient library.
#[derive(uniffi::Object)]
pub struct Gradients {
    file: Option<PathBuf>,
    kept: Mutex<Vec<GradientInfo>>,
}

#[uniffi::export]
impl Gradients {
    /// The gradients kept at `path` (or ours, when none are kept there yet).
    #[uniffi::constructor]
    pub fn open(path: String) -> Arc<Gradients> {
        let file = PathBuf::from(path);
        let kept = std::fs::read_to_string(&file).ok().and_then(|t| read(&t)).unwrap_or_else(ours);
        Arc::new(Gradients { file: Some(file), kept: Mutex::new(kept) })
    }

    pub fn list(&self) -> Vec<GradientInfo> {
        self.kept().clone()
    }

    /// The colour gradient `index` has at `p` (0..1), 0xRRGGBB.
    pub fn colour_at(&self, index: u32, p: f32) -> u32 {
        self.kept().get(index as usize).map_or(0, |g| colour_at(g, p))
    }

    /// A new gradient, black to white, just after `after`; returns where it went.
    pub fn add_after(&self, after: u32) -> u32 {
        let mut at = 0;
        self.change(|list| {
            at = (after as usize + 1).min(list.len());
            let stops =
                vec![GradientStop { position: 0.0, rgb: 0x000000 }, GradientStop { position: 1.0, rgb: 0xFFFFFF }];
            list.insert(at, GradientInfo { name: "Gradient".into(), stops });
        });
        at as u32
    }

    /// A copy of gradient `index` just after it; returns where it went.
    pub fn duplicate(&self, index: u32) -> u32 {
        self.change(|list| {
            if let Some(g) = list.get(index as usize).cloned() {
                list.insert(index as usize + 1, g);
            }
        });
        index + 1
    }

    /// Takes gradient `index` out (the library keeps one).
    pub fn delete(&self, index: u32) {
        self.change(|list| {
            if list.len() > 1 && (index as usize) < list.len() {
                list.remove(index as usize);
            }
        });
    }

    /// Renames it (a blank name is ignored).
    pub fn rename(&self, index: u32, name: String) {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.change(|list| {
            if let Some(g) = list.get_mut(index as usize) {
                g.name = name;
            }
        });
    }

    /// A stop at `position` (0..1), coloured as the gradient was there; returns its index.
    pub fn add_stop(&self, index: u32, position: f32) -> u32 {
        let mut at = 0;
        self.change(|list| {
            if let Some(g) = list.get_mut(index as usize) {
                let position = position.clamp(0.0, 1.0);
                let rgb = colour_at(g, position);
                g.stops.push(GradientStop { position, rgb });
                at = g.stops.len() - 1;
            }
        });
        at as u32
    }

    pub fn move_stop(&self, index: u32, stop: u32, position: f32) {
        self.change(|list| {
            if let Some(s) = list.get_mut(index as usize).and_then(|g| g.stops.get_mut(stop as usize)) {
                s.position = position.clamp(0.0, 1.0);
            }
        });
    }

    pub fn set_stop_colour(&self, index: u32, stop: u32, rgb: u32) {
        self.change(|list| {
            if let Some(s) = list.get_mut(index as usize).and_then(|g| g.stops.get_mut(stop as usize)) {
                s.rgb = rgb & 0xFF_FFFF;
            }
        });
    }

    /// Takes a stop out (a gradient keeps two).
    pub fn delete_stop(&self, index: u32, stop: u32) {
        self.change(|list| {
            if let Some(g) = list.get_mut(index as usize)
                && g.stops.len() > 2
                && (stop as usize) < g.stops.len()
            {
                g.stops.remove(stop as usize);
            }
        });
    }
}

impl Gradients {
    fn kept(&self) -> std::sync::MutexGuard<'_, Vec<GradientInfo>> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Changes the library and writes it out at once.
    fn change(&self, f: impl FnOnce(&mut Vec<GradientInfo>)) {
        let mut list = self.kept();
        f(&mut list);
        let Some(file) = &self.file else { return };
        let tmp = file.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, write(&list)).and_then(|()| std::fs::rename(&tmp, file)) {
            log::warn!("keeping the gradients: {e}");
        }
    }

    /// The gradient's stops as Gradient Map takes them: sorted, (position, r, g, b) each, 0..1.
    pub(crate) fn map_stops(g: &GradientInfo) -> Vec<f32> {
        let mut stops = g.stops.clone();
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        stops
            .iter()
            .flat_map(|s| {
                let ch = |shift: u32| ((s.rgb >> shift) & 0xFF) as f32 / 255.0;
                [s.position, ch(16), ch(8), ch(0)]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_reads_what_the_app_wrote_before() {
        let android = r#"{"gradients":[{"name":"Mine","stops":[[0,4278190080],[0.5,4294901760],[1,4294967295]]},{"name":"Short","stops":[[0,4278190080]]}]}"#;
        let list = read(android).expect("read");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].stops[1].rgb, 0xFF0000);
        assert_eq!(read(&write(&list)), Some(list));
        assert_eq!(read("{}"), None);
    }

    #[test]
    fn colours_between_stops_are_mixed() {
        let g = &ours()[4];
        assert_eq!(colour_at(g, 0.0), 0x000000);
        assert_eq!(colour_at(g, 1.0), 0xFFFFFF);
        assert_eq!(colour_at(g, 0.5), 0x7F7F7F);
        assert_eq!(Gradients::map_stops(g), vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn edits_keep_a_gradient_whole() {
        let dir = std::env::temp_dir().join(format!("procraft-gradients-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("gradients.json");
        let _ = std::fs::remove_file(&path);
        let lib = Gradients::open(path.to_string_lossy().into_owned());
        let at = lib.add_after(0);
        assert_eq!(at, 1);
        let stop = lib.add_stop(at, 0.25);
        assert_eq!(lib.list()[1].stops[stop as usize].rgb, 0x3F3F3F);
        lib.delete_stop(at, 0);
        lib.delete_stop(at, 0);
        assert_eq!(lib.list()[1].stops.len(), 2);
        lib.rename(at, "  ".into());
        assert_eq!(lib.list()[1].name, "Gradient");
        // what was written comes back
        let again = Gradients::open(path.to_string_lossy().into_owned());
        assert_eq!(again.list(), lib.list());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
