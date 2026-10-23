//! Colour profiles by name: the built-in RGB ones, ProCraft's own CMYK profile and those the user
//! imported (kept as .icc files in a folder the app names), and colours moved between them for
//! the colour panel.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use procraft_engine::cms::{CanvasIcc, IccProfile, Space};
use procraft_engine::document::Document;
use procraft_engine::profile::Profile;

use crate::CanvasSession;

/// The profiles the user imported, and the folder they are kept in.
struct Imported {
    folder: Option<PathBuf>,
    profiles: Vec<Arc<IccProfile>>,
}

static IMPORTED: Mutex<Imported> = Mutex::new(Imported { folder: None, profiles: Vec::new() });

fn imported() -> Vec<Arc<IccProfile>> {
    IMPORTED.lock().map(|i| i.profiles.clone()).unwrap_or_default()
}

/// ProCraft's own CMYK profile, read once.
fn generic_cmyk() -> Option<Arc<IccProfile>> {
    static READ: std::sync::OnceLock<Option<Arc<IccProfile>>> = std::sync::OnceLock::new();
    READ.get_or_init(|| IccProfile::parse(&procraft_engine::cmyk::profile()).map(Arc::new)).clone()
}

/// What a profile name puts a canvas in: a built-in profile, and a profile of its own if any.
pub(crate) fn resolve(name: &str) -> Option<(Profile, Option<CanvasIcc>)> {
    if let Some(p) = Profile::from_name(name) {
        return Some((p, None));
    }
    let icc = std::iter::once(generic_cmyk()).flatten().chain(imported()).find(|p| p.name == name)?;
    Some(placed(icc))
}

/// The built-in profile beside `icc`: Display P3 for a CMYK profile (its canvas's pixels are in
/// it), else the one nearest an RGB profile's name.
fn placed(icc: Arc<IccProfile>) -> (Profile, Option<CanvasIcc>) {
    let near = if icc.space == Space::Cmyk || icc.name.contains("P3") { Profile::DisplayP3 } else { Profile::Srgb };
    (near, Some(CanvasIcc(icc)))
}

/// The RGB profile named `name` (a canvas's own when it is the one named), if any.
fn rgb_named(name: &str, doc: Option<&Document>) -> Option<Arc<IccProfile>> {
    if let Some(d) = doc.filter(|d| d.profile_name() == name) {
        return Some(d.rgb_profile());
    }
    let (profile, icc) = resolve(name)?;
    let mut probe = Document::new(1, 1);
    probe.change_to(profile, icc);
    Some(probe.rgb_profile())
}

/// The table taking a canvas's colours to the screen's (as a press would print them, for a CMYK
/// canvas), for a canvas with a profile of its own; none for the built-in profiles, which the
/// display moves by matrix.
#[cfg(any(target_os = "android", test))]
pub(crate) fn display_table(doc: &Document, screen: Profile) -> Option<procraft_engine::cms::Lut> {
    doc.icc.as_ref()?;
    use procraft_engine::cms;
    Some(cms::Lut::build(&doc.rgb_profile(), &cms::builtin(screen), doc.press().as_deref(), 33))
}

/// What [`display_table`] depends on, to know when to make it again.
#[cfg(any(target_os = "android", test))]
pub(crate) fn table_key(doc: &Document, screen: Profile) -> Option<(usize, Profile)> {
    doc.icc.as_ref().map(|i| (Arc::as_ptr(&i.0) as usize, screen))
}

/// 0xRRGGBB as 0..1 values.
fn unpack(rgb: u32) -> [f64; 3] {
    [16, 8, 0].map(|s| f64::from((rgb >> s) & 0xFF) / 255.0)
}

/// 0..1 values as 0xRRGGBB.
fn pack(v: &[f64]) -> u32 {
    let byte = |k: usize| (v.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(0) << 16) | (byte(1) << 8) | byte(2)
}

/// Tells where imported profiles are kept, and reads those already there.
#[uniffi::export]
pub fn set_profile_folder(dir: String) {
    let folder = PathBuf::from(dir);
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if let Some(p) = std::fs::read(&path).ok().and_then(|b| IccProfile::parse(&b)).filter(|p| p.can_receive()) {
                profiles.push(Arc::new(p));
            }
        }
    }
    if let Ok(mut i) = IMPORTED.lock() {
        *i = Imported { folder: Some(folder), profiles };
    }
}

/// Adds the profile in `bytes` (an .icc file) to those a canvas can be in, keeping a copy; its
/// name, or none if it isn't an RGB or CMYK profile colours can be turned into. A profile with the
/// name of one there already takes its place.
#[uniffi::export]
pub fn import_profile(bytes: Vec<u8>) -> Option<String> {
    let p = IccProfile::parse(&bytes).filter(|p| p.can_receive() && p.space != Space::Gray && !p.name.is_empty())?;
    if Profile::from_name(&p.name).is_some() || generic_cmyk().is_some_and(|g| g.name == p.name) {
        return Some(p.name);
    }
    let mut i = IMPORTED.lock().ok()?;
    if let Some(folder) = &i.folder {
        let safe: String =
            p.name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
        let _ = std::fs::create_dir_all(folder);
        if let Err(e) = std::fs::write(folder.join(format!("{}.icc", safe.trim())), &bytes) {
            log::error!("keeping an imported profile: {e}");
        }
    }
    let name = p.name.clone();
    i.profiles.retain(|q| q.name != name);
    i.profiles.push(Arc::new(p));
    Some(name)
}

/// The CMYK profiles a canvas can print to, by name, in the order lists show them.
#[uniffi::export]
pub fn cmyk_profiles() -> Vec<String> {
    let ours = generic_cmyk().map(|p| p.name.clone());
    ours.into_iter().chain(imported().iter().filter(|p| p.space == Space::Cmyk).map(|p| p.name.clone())).collect()
}

/// The RGB profiles a canvas can be in, by name, in the order lists show them: the built-in ones,
/// then those imported.
#[uniffi::export]
pub fn color_profiles() -> Vec<String> {
    Profile::ALL
        .iter()
        .map(|p| p.name().to_owned())
        .chain(imported().iter().filter(|p| p.space == Space::Rgb).map(|p| p.name.clone()))
        .collect()
}

/// Whether `name` is one of the built-in RGB profiles (whose colours the screen shows by matrix).
#[uniffi::export]
pub fn profile_is_built_in(name: String) -> bool {
    Profile::from_name(&name).is_some()
}

/// Whether the profile named `name` is a CMYK one.
#[uniffi::export]
pub fn is_cmyk_profile(name: String) -> bool {
    resolve(&name).is_some_and(|(_, icc)| icc.is_some_and(|i| i.0.space == Space::Cmyk))
}

/// An 8-bit colour (0xRRGGBB) in the RGB profile named `from` (a CMYK profile's: Display P3), as it
/// is in `to`; unchanged if either name isn't a profile.
#[uniffi::export]
pub fn convert_color(rgb: u32, from: String, to: String) -> u32 {
    match (Profile::from_name(&from), Profile::from_name(&to)) {
        (Some(from), Some(to)) => from.convert_rgb(rgb & 0xFF_FFFF, to),
        _ => match (rgb_named(&from, None), rgb_named(&to, None)) {
            (Some(a), Some(b)) => pack(&a.convert(&unpack(rgb), &b)),
            _ => rgb,
        },
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Whether the canvas prints to a CMYK profile.
    pub fn is_cmyk(&self) -> bool {
        self.shared.lock().doc.press().is_some()
    }

    /// The canvas colour `rgb` (0xRRGGBB) as the canvas shows it: as its press would print it on a
    /// CMYK canvas, else unchanged.
    pub fn proof_color(&self, rgb: u32) -> u32 {
        let st = self.shared.lock();
        let Some(press) = st.doc.press() else { return rgb & 0xFF_FFFF };
        let rgb_profile = st.doc.rgb_profile();
        pack(&press.convert(&rgb_profile.convert(&unpack(rgb), &press), &rgb_profile))
    }

    /// The inks (cyan, magenta, yellow, black; 0..1) a CMYK canvas prints canvas colour `rgb` with;
    /// empty on an RGB canvas.
    pub fn cmyk_of(&self, rgb: u32) -> Vec<f32> {
        let st = self.shared.lock();
        let Some(press) = st.doc.press() else { return Vec::new() };
        // white is the paper
        if rgb & 0xFF_FFFF == 0xFF_FFFF {
            return vec![0.0; 4];
        }
        st.doc.rgb_profile().convert(&unpack(rgb), &press).iter().map(|&v| v as f32).collect()
    }

    /// The canvas colour (0xRRGGBB) inks `cmyk` (0..1 each) print as on a CMYK canvas; black on an
    /// RGB canvas.
    pub fn rgb_of_cmyk(&self, cmyk: Vec<f32>) -> u32 {
        let st = self.shared.lock();
        let Some(press) = st.doc.press() else { return 0 };
        let inks: Vec<f64> = (0..4).map(|k| f64::from(cmyk.get(k).copied().unwrap_or(0.0)).clamp(0.0, 1.0)).collect();
        pack(&press.convert(&inks, &st.doc.rgb_profile()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_cmyk_profile_is_offered_and_resolves() {
        assert_eq!(cmyk_profiles().first().map(String::as_str), Some(procraft_engine::cmyk::NAME));
        let (profile, icc) = resolve(procraft_engine::cmyk::NAME).unwrap();
        assert_eq!(profile, Profile::DisplayP3);
        assert_eq!(icc.unwrap().0.space, Space::Cmyk);
        assert!(is_cmyk_profile(procraft_engine::cmyk::NAME.to_owned()));
        assert!(!is_cmyk_profile("Display P3".to_owned()));
        assert_eq!(resolve("sRGB IEC61966-2.1"), Some((Profile::Srgb, None)));
        assert!(resolve("no such profile").is_none());
    }

    #[test]
    fn a_cmyk_canvas_shows_through_a_table() {
        let mut doc = Document::new(4, 4);
        assert!(display_table(&doc, Profile::Srgb).is_none() && table_key(&doc, Profile::Srgb).is_none());
        let (profile, icc) = resolve(procraft_engine::cmyk::NAME).unwrap();
        doc.change_to(profile, icc);
        let table = display_table(&doc, Profile::DisplayP3).unwrap();
        // the press dulls a pure green; white stays white
        let green = table.apply([0.0, 1.0, 0.0]);
        assert!(green[0] > 0.2 && green[1] < 0.9, "{green:?}");
        assert_eq!(table.apply([1.0, 1.0, 1.0]), vec![1.0, 1.0, 1.0]);
        assert_eq!(table_key(&doc, Profile::DisplayP3), table_key(&doc.clone(), Profile::DisplayP3));
    }

    #[test]
    fn colours_move_between_profiles_by_name() {
        // built in: the matrices
        let p3_red = convert_color(0xFF0000, "sRGB IEC61966-2.1".to_owned(), "Display P3".to_owned());
        assert_eq!(p3_red, Profile::Srgb.convert_rgb(0xFF0000, Profile::DisplayP3));
        // a CMYK profile's colours are Display P3's
        let same = convert_color(0x12AB34, "Display P3".to_owned(), procraft_engine::cmyk::NAME.to_owned());
        assert!((same >> 16).abs_diff(0x12) <= 1 && ((same >> 8) & 0xFF).abs_diff(0xAB) <= 1, "{same:06x}");
        assert_eq!(convert_color(0x123456, "nothing".to_owned(), "Display P3".to_owned()), 0x123456);
    }
}
