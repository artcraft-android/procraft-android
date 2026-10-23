//! The brush settings as the brush studio shows them: each control by its key, holding what the
//! control shows (0..1; −1..1 for the controls that go both ways; 0 or 1 for a switch; the index
//! for a choice). Controls the engine does not draw yet keep their values with the brush.

use std::f32::consts::PI;

use crate::brush::{Brush, FLOW_POWER, GrainMode, Rendering};
use crate::document::BlendMode;

/// The most size (in brush-file units, see [`Brush::size_unit`]) the size controls can reach, for
/// a brush spaced at least [`ROOMY_SPACING`] apart; a control's place is the size's share of the
/// brush's own most size (see [`Brush::most_size`]) to the power 1/2.2.
pub const MOST_SIZE: f32 = 16.0;
/// The spacing (as files keep it) from which a brush may be as big as [`MOST_SIZE`]; closer
/// spaced brushes are kept smaller, down to 1 unit at none.
pub const ROOMY_SPACING: f32 = 0.02;
const SIZE_CONTROL_CURVE: f32 = 2.2;
/// The largest grain tile (canvas pixels) the grain scale control reaches.
pub const MAX_GRAIN_SCALE: f32 = 1024.0;
/// The smallest grain tile, canvas pixels.
const LEAST_GRAIN_SCALE: f32 = 4.0;
/// The most stamps placed at each step.
pub const MAX_COUNT: u32 = 16;

/// Where the controls kept in the brush's `extra` start (the others start where the brush is);
/// any control not listed starts at 0.
const STARTS: &[(&str, f32)] = &[
    ("pen_tip_animation", 1.0),
    // leaning grows the stamp only as the stroke runs across the lean; the stamp as round as
    // its shape until the roundness tilt takes it
    ("tilt_size_compression", 1.0),
    ("tilt_roundness_max", 1.0),
    // the newer spacing (see [`Brush::spacing`])
    ("spacing_version", 1.0),
    ("classic_taper", 1.0),
    ("shape_filtering", 2.0),
    ("grain_movement", 1.0),
    ("grain_zoom", 1.0),
    // grain and burnt edges multiply
    ("grain_blend", 1.0),
    ("burnt_blend", 1.0),
    ("grain_offset_jitter", 1.0),
    ("grain_filtering", 2.0),
    ("grain_follows_camera", 1.0),
    ("wet_pull", 0.75),
    ("tilt_size_compression", 1.0),
    ("hover_pressure", 0.44),
    ("hover_fill", 1.0),
    ("smudge_pull", 0.75),
    ("preview_size", 0.09),
    // the control's middle: the file's 1, of 0 to 2
    ("preview_pressure_scale", 0.5),
    // the hover outline in contrasting rings
    ("cursor_outline", 1.0),
];

/// The controls drawn from the brush's own fields (the others are kept in its `extra`).
pub const DRAWN: [&str; 37] = [
    "spacing",
    "streamline",
    "streamline_pressure",
    "pen_taper_start",
    "pen_taper_end",
    "pen_taper_size",
    "pen_taper_opacity",
    "touch_taper_start",
    "touch_taper_end",
    "touch_taper_size",
    "touch_taper_opacity",
    "input_style",
    "shape_rotation",
    "scatter",
    "count",
    "randomized",
    "roundness",
    "shape_angle",
    "grain_behavior",
    "grain_scale",
    "grain_depth",
    "rendering_mode",
    "flow",
    "jitter_size",
    "jitter_opacity",
    "pressure_size",
    "pressure_opacity",
    "tilt_angle",
    "tilt_opacity",
    "tilt_size",
    "max_size",
    "min_size",
    "max_opacity",
    "min_opacity",
    "smudge_pull",
    "preview_size",
    "wet_pull",
];

/// Spacing as the studio's control shows it: the gap between stamp centres, in stamp diameters,
/// rises with the control to the power [`SPACING_CURVE`], from [`SPACING_FLOOR`] at none to almost
/// three diameters at the full control (so most of the control is spent on close spacings).
pub(crate) const SPACING_FLOOR: f32 = 0.0083;
pub(crate) const SPACING_SPAN: f32 = 2.0 * std::f32::consts::SQRT_2;
const SPACING_CURVE: f32 = 2.2;

impl Brush {
    /// The most size (brush-file units) the brush may have: [`MOST_SIZE`] when spaced at least
    /// [`ROOMY_SPACING`] apart, less the closer its stamps are (so a stroke stays a manageable
    /// number of stamps), 1 at no spacing.
    pub fn most_size(&self) -> f32 {
        let spacing = ((self.spacing - SPACING_FLOOR) / (SPACING_SPAN / 2.0)).max(0.0);
        if spacing <= 0.0 { 1.0 } else { 1.0 + (MOST_SIZE - 1.0) * (spacing / ROOMY_SPACING).min(1.0) }
    }

    /// Whether the brush mixes its paint wet with what is under it: a blending rendering mode
    /// (Uniform or Intense Blending; glazes never mix) with some dilution (without water the
    /// other wet settings do nothing).
    pub fn mixes_wet(&self) -> bool {
        self.setting("wet_dilution") > 0.0 && self.setting("rendering_mode") >= 3.5
    }

    /// Whether the brush blends (Uniform or Intense Blending, mixing as it goes): its opacity
    /// slider lightens each stamp instead of capping the stroke.
    pub fn blends(&self) -> bool {
        self.setting("rendering_mode") >= 3.5
    }

    fn has_wet_mix(&self) -> bool {
        self.extra.contains_key("wet_pull") || self.setting("wet_charge") > 0.0 || self.setting("wet_dilution") > 0.0
    }

    /// A size control's place for `size` canvas pixels.
    fn size_control(&self, size: f32) -> f32 {
        (size / self.size_unit() / self.most_size()).clamp(0.0, 1.0).powf(1.0 / SIZE_CONTROL_CURVE)
    }

    /// The size (canvas pixels) at a size control's place.
    fn size_from_control(&self, place: f32) -> f32 {
        self.most_size() * place.clamp(0.0, 1.0).powf(SIZE_CONTROL_CURVE) * self.size_unit()
    }
}

fn flag(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

impl Brush {
    /// The value the control `key` shows.
    pub fn setting(&self, key: &str) -> f32 {
        match key {
            "spacing" => ((self.spacing - SPACING_FLOOR).max(0.0) / SPACING_SPAN).powf(1.0 / SPACING_CURVE),
            "streamline" => self.streamline,
            "streamline_pressure" => self.pressure_smoothing,
            "pen_taper_start" => self.pen_taper.start,
            "pen_taper_end" => self.pen_taper.end,
            "pen_taper_size" => self.pen_taper.size,
            "pen_taper_opacity" => self.pen_taper.opacity,
            "touch_taper_start" => self.touch_taper.start,
            "touch_taper_end" => self.touch_taper.end,
            "touch_taper_size" => self.touch_taper.size,
            "touch_taper_opacity" => self.touch_taper.opacity,
            "input_style" => self.extra_or("input_style", flag(self.shape.follow_azimuth)),
            "shape_rotation" => self.shape.follow,
            "scatter" => self.shape.scatter / 2.0,
            "count" => (self.shape.count.clamp(1, MAX_COUNT) - 1) as f32 / (MAX_COUNT - 1) as f32,
            "randomized" => flag(self.shape.randomise),
            "roundness" => self.shape.roundness,
            // the shape's own turn, as a share of a full turn
            "shape_angle" => (self.shape.rotation / std::f32::consts::TAU).rem_euclid(1.0),
            "grain_behavior" => flag(self.grain.mode == GrainMode::Texturized),
            // the smallest tile reads as none (a file's 0)
            "grain_scale" => {
                if self.grain.scale <= LEAST_GRAIN_SCALE {
                    0.0
                } else {
                    self.grain.scale / MAX_GRAIN_SCALE
                }
            }
            "grain_depth" => self.grain.depth,
            "rendering_mode" => {
                // a brush of ours (no mode kept) as the mode it draws like
                let drawn = match self.rendering {
                    Rendering::Strongest => 0.0,
                    Rendering::BuildUp if self.has_wet_mix() => 5.0,
                    Rendering::BuildUp => 2.0,
                    Rendering::Gradual => 3.0,
                    Rendering::Capped => 1.0,
                };
                self.extra_or("rendering_mode", drawn)
            }
            "flow" => self.flow.max(0.0).powf(1.0 / FLOW_POWER),
            "jitter_size" => self.shape.size_jitter,
            "jitter_opacity" => self.shape.opacity_jitter,
            "pressure_size" => self.pressure_size,
            "pressure_opacity" => self.pressure_opacity,
            "tilt_angle" => self.tilt.angle / (PI / 2.0),
            // each tilt control's own angle, else the tilt angle's
            "opacity_tilt_angle"
            | "size_tilt_angle"
            | "gradation_tilt_angle"
            | "bleed_tilt_angle"
            | "roundness_tilt_angle"
            | "hue_tilt_angle"
            | "saturation_tilt_angle"
            | "brightness_tilt_angle"
            | "secondary_tilt_angle" => self.extra_or(key, self.tilt.angle / (PI / 2.0)),
            "tilt_opacity" => self.tilt.opacity,
            "tilt_size" => self.tilt.size,
            "max_size" => self.size_control(self.max_size),
            // read only: the size the size controls reach at their end, for showing them
            "most_size" => self.most_size(),
            "min_size" => self.size_control(self.min_size),
            "max_opacity" => self.max_opacity,
            "min_opacity" => self.min_opacity,
            _ => self.extra_or(key, STARTS.iter().find(|(k, _)| *k == key).map_or(0.0, |&(_, v)| v)),
        }
    }

    /// Sets each control of `settings`, the sizes last: the spacing sets how big the brush may be,
    /// so where their controls sit.
    pub fn set_settings<'a>(&mut self, settings: impl IntoIterator<Item = (&'a str, f32)>) {
        let (sizes, rest): (Vec<_>, Vec<_>) =
            settings.into_iter().partition(|(k, _)| matches!(*k, "max_size" | "min_size"));
        for (k, v) in rest.into_iter().chain(sizes) {
            self.set_setting(k, v);
        }
    }

    /// Sets the control `key` to `value` (kept to the control's range); not a number does nothing.
    pub fn set_setting(&mut self, key: &str, value: f32) {
        if !value.is_finite() {
            return;
        }
        let unit = value.clamp(0.0, 1.0);
        let both = value.clamp(-1.0, 1.0);
        let on = value >= 0.5;
        match key {
            "spacing" => self.spacing = SPACING_FLOOR + SPACING_SPAN * unit.powf(SPACING_CURVE),
            "streamline" => self.streamline = unit,
            "streamline_pressure" => self.pressure_smoothing = unit,
            "pen_taper_start" => self.pen_taper.start = unit,
            "pen_taper_end" => self.pen_taper.end = unit,
            "pen_taper_size" => self.pen_taper.size = unit,
            "pen_taper_opacity" => self.pen_taper.opacity = unit,
            "touch_taper_start" => self.touch_taper.start = unit,
            "touch_taper_end" => self.touch_taper.end = unit,
            "touch_taper_size" => self.touch_taper.size = unit,
            "touch_taper_opacity" => self.touch_taper.opacity = unit,
            "input_style" => {
                // touch only, azimuth, or azimuth and barrel roll (which turns with the azimuth)
                let style = value.round().clamp(0.0, 2.0);
                self.shape.follow_azimuth = style >= 1.0;
                self.extra.insert(key.to_owned(), style);
            }
            // how far the stamp turns with the stroke (its fixed angle is the shape's own)
            "shape_rotation" => self.shape.follow = both,
            "scatter" => self.shape.scatter = unit * 2.0,
            "count" => self.shape.count = 1 + (unit * (MAX_COUNT - 1) as f32).round() as u32,
            "randomized" => self.shape.randomise = on,
            "roundness" => self.shape.roundness = unit.max(0.05),
            "shape_angle" => self.shape.rotation = value.rem_euclid(1.0) * std::f32::consts::TAU,
            "grain_behavior" => self.grain.mode = if on { GrainMode::Texturized } else { GrainMode::Moving },
            "grain_scale" => self.grain.scale = (unit * MAX_GRAIN_SCALE).max(LEAST_GRAIN_SCALE),
            "grain_depth" => self.grain.depth = unit,
            "rendering_mode" => {
                // Light Glaze keeps the strongest stamp; Heavy Glaze and Uniform Blending build up
                // gradually; the others build up
                let mode = value.round().clamp(0.0, 5.0);
                self.rendering = match mode as u8 {
                    0 => Rendering::Strongest,
                    1 => Rendering::Capped,
                    3 | 4 => Rendering::Gradual,
                    _ => Rendering::BuildUp,
                };
                self.extra.insert(key.to_owned(), mode);
            }
            // blend modes are kept by number (as the layers' are numbered)
            "blend_mode" | "grain_blend" | "burnt_blend" => {
                self.extra.insert(key.to_owned(), value.round().clamp(0.0, BlendMode::MAX_ID as f32));
            }
            "flow" => self.flow = unit.powf(FLOW_POWER),
            "jitter_size" => self.shape.size_jitter = unit,
            "jitter_opacity" => self.shape.opacity_jitter = unit,
            "pressure_size" => self.pressure_size = both,
            "pressure_opacity" => self.pressure_opacity = both,
            "tilt_angle" => self.tilt.angle = unit * PI / 2.0,
            "tilt_opacity" => self.tilt.opacity = unit,
            "tilt_size" => self.tilt.size = unit,
            // the least size is kept no larger than the most
            "max_size" => {
                self.max_size = self.size_from_control(unit).max(0.5);
                self.min_size = self.min_size.min(self.max_size);
            }
            "min_size" => self.min_size = self.size_from_control(unit).min(self.max_size),
            "max_opacity" => self.max_opacity = unit,
            "min_opacity" => self.min_opacity = unit,
            // choices of three, kept by their place in the list
            "shape_filtering" | "grain_filtering" | "cursor_outline" | "hover_fill" | "relative_to_stroke" => {
                self.extra.insert(key.to_owned(), value.round().clamp(0.0, 2.0));
            }
            _ => {
                self.extra.insert(key.to_owned(), both);
            }
        }
    }

    /// Every control whose value differs from `shipped`'s, with its value: those drawn from the
    /// brush's fields and those kept with it.
    pub fn changes_from(&self, shipped: &Brush) -> Vec<(String, f32)> {
        let mut keys: Vec<&str> = DRAWN.to_vec();
        keys.extend(self.extra.keys().map(String::as_str));
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|k| (k, self.setting(k)))
            .filter(|&(k, v)| (v - shipped.setting(k)).abs() > 1e-6)
            .map(|(k, v)| (k.to_owned(), v))
            .collect()
    }

    fn extra_or(&self, key: &str, start: f32) -> f32 {
        self.extra.get(key).copied().unwrap_or(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_round_trip() {
        let mut b = Brush::round_pen();
        for (key, v) in [
            ("spacing", 0.3),
            ("streamline", 0.66),
            ("pen_taper_size", 0.12),
            ("shape_rotation", -0.5),
            ("scatter", 0.25),
            ("roundness", 0.4),
            ("grain_scale", 0.25),
            ("flow", 0.8),
            ("pressure_size", -0.3),
            ("tilt_angle", 0.5),
            ("max_size", 0.23),
            ("min_size", 0.02),
            ("max_opacity", 0.9),
            ("wet_pull", 0.4),
            ("hue_jitter", 0.1),
        ] {
            b.set_setting(key, v);
            assert!((b.setting(key) - v).abs() < 1e-4, "{key}: {} != {v}", b.setting(key));
        }
        b.set_setting("count", 3.0 / 15.0);
        assert_eq!(b.shape.count, 4);
        b.set_setting("rendering_mode", 4.0);
        assert_eq!((b.rendering, b.setting("rendering_mode")), (Rendering::Gradual, 4.0));
        b.set_setting("grain_behavior", 1.0);
        assert_eq!(b.grain.mode, GrainMode::Texturized);
        // the smallest grain tile reads as none, as set
        b.set_setting("grain_scale", 0.0);
        assert_eq!((b.grain.scale, b.setting("grain_scale")), (LEAST_GRAIN_SCALE, 0.0));
        b.set_setting("spacing", f32::NAN);
        assert!((b.setting("spacing") - 0.3).abs() < 1e-6);
        // where not set, controls start where the brush is, or at their own start
        assert_eq!(Brush::round_pen().setting("smudge_pull"), 0.75);
        assert_eq!(Brush::round_pen().setting("nothing_here"), 0.0);
    }

    #[test]
    fn spacing_follows_its_curve() {
        let mut b = Brush::round_pen();
        b.set_setting("spacing", 0.0);
        assert!((b.spacing - SPACING_FLOOR).abs() < 1e-6);
        b.set_setting("spacing", 1.0);
        assert!((b.spacing - SPACING_FLOOR - SPACING_SPAN).abs() < 1e-5);
        // three quarters of the control: stamps about 1.5 diameters apart, round dots with gaps
        b.set_setting("spacing", 0.75);
        assert!((b.spacing - 1.51).abs() < 0.01, "{}", b.spacing);
        // a spacing tighter than the floor shows as none
        b.spacing = 0.001;
        assert_eq!(b.setting("spacing"), 0.0);
    }

    #[test]
    fn changes_replay_onto_the_shipped_brush() {
        let shipped = Brush::round_pen();
        let mut b = shipped.clone();
        b.set_setting("spacing", 0.4);
        b.set_setting("max_size", 0.6);
        b.set_setting("min_size", 0.2);
        b.set_setting("wet_attack", 0.3);
        let changes = b.changes_from(&shipped);
        assert!(changes.iter().any(|(k, _)| k == "wet_attack"));
        assert!(!changes.iter().any(|(k, _)| k == "flow"));
        let mut again = shipped.clone();
        for (k, v) in &changes {
            again.set_setting(k, *v);
        }
        assert!(again.changes_from(&b).is_empty(), "{:?}", again.changes_from(&b));
    }

    #[test]
    fn close_spacing_keeps_brushes_smaller() {
        let mut b = Brush::round_pen();
        // a file's spacing of 0.0064 lets a brush be 5.8 units at most; none, 1
        b.set_setting("spacing", (0.0064f32 / 2.0).powf(1.0 / 2.2));
        assert!((b.most_size() - 5.8).abs() < 1e-3, "{}", b.most_size());
        b.set_setting("spacing", 0.0);
        assert_eq!(b.most_size(), 1.0);
        // and its size is held to it
        b.max_size = 10.0 * b.size_unit();
        assert!((b.diameter(1.0) - b.size_unit()).abs() < 1e-3);
    }

    #[test]
    fn choices_of_three_keep_their_last() {
        let mut b = Brush::round_pen();
        for key in ["shape_filtering", "grain_filtering", "cursor_outline", "hover_fill"] {
            b.set_setting(key, 2.0);
            assert_eq!(b.setting(key), 2.0, "{key}");
            b.set_setting(key, 7.0);
            assert_eq!(b.setting(key), 2.0, "{key}");
        }
    }

    #[test]
    fn the_size_controls_share_one_scale() {
        let mut b = Brush::round_pen();
        // the end of the control is the most size; a quarter of a unit sits at (1/64)^(1/2.2)
        // spaced roomily, the end of the control is the most size; a quarter of a unit sits at
        // (1/64)^(1/2.2)
        b.set_setting("spacing", 0.5);
        assert_eq!(b.most_size(), MOST_SIZE);
        b.set_setting("max_size", 1.0);
        assert!((b.max_size - MOST_SIZE * b.size_unit()).abs() < 1e-2);
        b.set_setting("max_size", (0.25f32 / MOST_SIZE).powf(1.0 / 2.2));
        assert!((b.max_size - 0.25 * b.size_unit()).abs() < 1e-3);
        // the least size is its own, not a share of the most, and no larger than it
        b.set_setting("min_size", (0.1f32 / MOST_SIZE).powf(1.0 / 2.2));
        assert!((b.min_size - 0.1 * b.size_unit()).abs() < 1e-3);
        b.set_setting("max_size", 1.0);
        assert!((b.min_size - 0.1 * b.size_unit()).abs() < 1e-3);
        b.set_setting("min_size", 1.0);
        assert_eq!(b.min_size, b.max_size);
    }
}
