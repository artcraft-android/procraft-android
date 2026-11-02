//! The canvases to start from in the gallery's New canvas list, the same in every app: each one's
//! name, size in its unit, DPI, background, time-lapse video and colour profile; the list a new
//! gallery starts with; and the list as each app keeps it (a JSON array).

use crate::colour::{cmyk_profiles, is_cmyk_profile};

/// The longest side a canvas can have, pixels.
pub const MAX_SIDE: u32 = 16384;
pub const SRGB: &str = "sRGB IEC61966-2.1";
pub const P3: &str = "Display P3";

/// A canvas to start from: its size in `unit` ("px", "mm", "cm" or "in"), DPI, background
/// (0xRRGGBB, and whether it is hidden), time-lapse video (resolution 0..2 for 1080p, 2K and 4K;
/// quality 0..3; HEVC) and colour profile, by name.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CanvasPreset {
    pub name: String,
    pub unit: String,
    pub size_width: f32,
    pub size_height: f32,
    pub dpi: f32,
    pub background: u32,
    pub background_hidden: bool,
    pub time_lapse_resolution: u32,
    pub time_lapse_quality: u32,
    pub time_lapse_hevc: bool,
    pub profile: String,
}

impl CanvasPreset {
    fn new(name: &str, unit: &str, size: (f32, f32), dpi: f32, profile: &str) -> CanvasPreset {
        CanvasPreset {
            name: name.to_owned(),
            unit: unit.to_owned(),
            size_width: size.0,
            size_height: size.1,
            dpi,
            background: 0xFF_FFFF,
            background_hidden: false,
            time_lapse_resolution: 0,
            time_lapse_quality: 1,
            time_lapse_hevc: false,
            profile: profile.to_owned(),
        }
    }

    fn pixels(&self, v: f32) -> u32 {
        let px = per_inch(self.unit.clone()).map_or(v, |k| v / k * self.dpi);
        (px.round().max(1.0) as u32).min(MAX_SIDE).min(procraft_engine::document::canvas_edge_most())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "unit": self.unit,
            "width": self.size_width,
            "height": self.size_height,
            "dpi": self.dpi,
            "background": self.background,
            "hidden": self.background_hidden,
            "resolution": self.time_lapse_resolution,
            "quality": self.time_lapse_quality,
            "hevc": self.time_lapse_hevc,
            "profile": self.profile,
        })
    }

    fn from_json(j: &serde_json::Value) -> CanvasPreset {
        let text = |k: &str, or: &str| j.get(k).and_then(serde_json::Value::as_str).unwrap_or(or).to_owned();
        let number = |k: &str, or: f64| j.get(k).and_then(serde_json::Value::as_f64).unwrap_or(or) as f32;
        let whole = |k: &str, or: u64| j.get(k).and_then(serde_json::Value::as_u64).unwrap_or(or) as u32;
        let flag = |k: &str| j.get(k).and_then(serde_json::Value::as_bool).unwrap_or(false);
        CanvasPreset {
            name: text("name", "Untitled Canvas"),
            unit: text("unit", "px"),
            size_width: number("width", 1080.0),
            size_height: number("height", 1080.0),
            dpi: number("dpi", 300.0),
            background: whole("background", 0xFF_FFFF),
            background_hidden: flag("hidden"),
            time_lapse_resolution: whole("resolution", 0),
            time_lapse_quality: whole("quality", 1),
            time_lapse_hevc: flag("hevc"),
            profile: text("profile", SRGB),
        }
    }
}

/// How many of `unit` make an inch; none for pixels.
#[uniffi::export]
pub fn per_inch(unit: String) -> Option<f32> {
    match unit.as_str() {
        "mm" => Some(25.4),
        "cm" => Some(2.54),
        "in" => Some(1.0),
        _ => None,
    }
}

/// `v` with at most two decimals and no trailing zeros.
#[uniffi::export]
pub fn preset_number(v: f32) -> String {
    // halves round up, as the apps have always shown them
    let v = (v * 100.0).round() / 100.0;
    format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The preset's width in pixels (1 to [`MAX_SIDE`]).
#[uniffi::export]
pub fn preset_width(preset: CanvasPreset) -> u32 {
    preset.pixels(preset.size_width)
}

#[uniffi::export]
pub fn preset_height(preset: CanvasPreset) -> u32 {
    preset.pixels(preset.size_height)
}

/// The size as the list shows it, in its unit.
#[uniffi::export]
pub fn preset_label(preset: CanvasPreset) -> String {
    let (w, h) = (preset_number(preset.size_width), preset_number(preset.size_height));
    if preset.unit == "in" {
        format!("{w}\u{2033} \u{d7} {h}\u{2033}")
    } else {
        format!("{w} \u{d7} {h}{}", preset.unit)
    }
}

/// A colour profile as a list's chip shows it.
#[uniffi::export]
pub fn profile_chip(profile: String) -> String {
    if profile == P3 {
        "P3".into()
    } else if is_cmyk_profile(profile) {
        "CMYK".into()
    } else {
        "sRGB".into()
    }
}

/// The presets a new gallery starts with.
#[uniffi::export]
pub fn standard_presets() -> Vec<CanvasPreset> {
    let comic = cmyk_profiles().into_iter().next().unwrap_or_else(|| SRGB.to_owned());
    vec![
        CanvasPreset::new("Square", "px", (2048.0, 2048.0), 132.0, SRGB),
        CanvasPreset::new("4K", "px", (4096.0, 1714.0), 132.0, SRGB),
        CanvasPreset::new("A4", "mm", (210.0, 297.0), 300.0, SRGB),
        CanvasPreset::new("4 \u{d7} 6 Photo", "in", (6.0, 4.0), 300.0, SRGB),
        CanvasPreset::new("Paper", "in", (11.0, 8.5), 300.0, SRGB),
        // printed: CMYK
        CanvasPreset::new("Comic", "in", (6.0, 9.5), 400.0, &comic),
    ]
}

/// The presets kept as `saved` (a JSON array), or the standard ones if none were (or they can't
/// be read).
#[uniffi::export]
pub fn presets_from_json(saved: Option<String>) -> Vec<CanvasPreset> {
    let read = saved.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    match read.as_ref().and_then(serde_json::Value::as_array) {
        Some(list) => list.iter().map(CanvasPreset::from_json).collect(),
        None => standard_presets(),
    }
}

/// `presets` as kept: a JSON array.
#[uniffi::export]
pub fn presets_json(presets: Vec<CanvasPreset>) -> String {
    serde_json::Value::Array(presets.iter().map(CanvasPreset::to_json).collect()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_come_out_in_pixels_and_as_the_list_shows_them() {
        let list = standard_presets();
        let a4 = list.iter().find(|p| p.name == "A4").unwrap();
        assert_eq!((preset_width(a4.clone()), preset_height(a4.clone())), (2480, 3508));
        assert_eq!(preset_label(a4.clone()), "210 \u{d7} 297mm");
        let paper = list.iter().find(|p| p.name == "Paper").unwrap();
        assert_eq!(preset_label(paper.clone()), "11\u{2033} \u{d7} 8.5\u{2033}");
        assert_eq!((preset_width(paper.clone()), preset_height(paper.clone())), (3300, 2550));
        let square = list.first().unwrap();
        assert_eq!(preset_label(square.clone()), "2048 \u{d7} 2048px");
        assert_eq!(profile_chip(list.last().unwrap().profile.clone()), "CMYK");
        assert_eq!(profile_chip(P3.into()), "P3");
        assert_eq!(profile_chip(SRGB.into()), "sRGB");
        // kept within what a canvas can be
        let huge = CanvasPreset { size_width: 1e9, size_height: 0.0, ..square.clone() };
        assert_eq!((preset_width(huge.clone()), preset_height(huge)), (MAX_SIDE, 1));
        assert_eq!(preset_number(10.5), "10.5");
        assert_eq!(preset_number(2.0), "2");
        assert_eq!(preset_number(0.125), "0.13");
    }

    #[test]
    fn a_kept_list_comes_back_as_it_was() {
        let mut list = standard_presets();
        list.push(CanvasPreset {
            background: 0x123456,
            background_hidden: true,
            time_lapse_resolution: 2,
            time_lapse_quality: 3,
            time_lapse_hevc: true,
            profile: P3.into(),
            ..CanvasPreset::new("Mine", "cm", (10.5, 20.0), 150.0, SRGB)
        });
        assert_eq!(presets_from_json(Some(presets_json(list.clone()))), list);
        assert_eq!(presets_from_json(None), standard_presets());
        assert_eq!(presets_from_json(Some("not json".into())), standard_presets());
        // as the Android app kept them before: missing keys take their defaults
        let old = presets_from_json(Some(r#"[{"name":"Old","unit":"px","width":800,"height":600}]"#.into()));
        assert_eq!(old.first().map(|p| (p.dpi, p.background, p.profile.as_str())), Some((300.0, 0xFF_FFFF, SRGB)));
    }
}
