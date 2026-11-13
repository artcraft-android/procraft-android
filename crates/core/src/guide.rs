//! The drawing guide: set from the guide editor, drawn over the canvas as lines, and followed by
//! strokes with assisted drawing on (kept to the guide's directions, or mirrored by symmetry).

use procraft_engine::geom::{Affine, Vec2};
use procraft_engine::guide::{Guide, GuideKind, LengthUnit, SymmetryKind, bar_color};
use procraft_engine::input::Sample;
use procraft_engine::stroke::Stamp;
use procraft_engine::view::View;

use crate::{CanvasSession, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum GuideType {
    Grid,
    Isometric,
    Perspective,
    Symmetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SymmetryType {
    Vertical,
    Horizontal,
    Quadrant,
    Radial,
}

/// The units a grid's size is typed and shown in, as its keypad lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum GuideUnit {
    Millimeters,
    Centimeters,
    Inches,
    Pixels,
}

/// The sizes a grid can have, canvas pixels.
const SIZE_MIN: f32 = 4.0;
const SIZE_MAX: f32 = 1000.0;

/// The guide as the editor and the overlay show it. Points are window pixels, x and y in turn;
/// `lines` are segments (two points each); `color` is sRGB 0..1.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GuideInfo {
    pub on: bool,
    pub kind: GuideType,
    pub opacity: f32,
    pub thickness: f32,
    /// The grid's size, canvas pixels; the unit it is shown in; and the size in that unit as its
    /// keypad starts with it ("71", "8", "0.5") and as its pill shows it ("71px", "8mm", "1″").
    pub size: f32,
    pub unit: GuideUnit,
    pub size_text: String,
    pub size_label: String,
    /// Where the colour sits on the colour bar (0..1), and the colour.
    pub hue: f32,
    pub color: Vec<f32>,
    pub symmetry: SymmetryType,
    pub rotational: bool,
    pub assisted: bool,
    pub centre: Vec<f32>,
    pub handle: Vec<f32>,
    pub vanishing: Vec<f32>,
    pub lines: Vec<f32>,
}

fn kind_of(t: GuideType) -> GuideKind {
    match t {
        GuideType::Grid => GuideKind::Grid,
        GuideType::Isometric => GuideKind::Isometric,
        GuideType::Perspective => GuideKind::Perspective,
        GuideType::Symmetry => GuideKind::Symmetry,
    }
}

fn type_of(k: GuideKind) -> GuideType {
    match k {
        GuideKind::Grid => GuideType::Grid,
        GuideKind::Isometric => GuideType::Isometric,
        GuideKind::Perspective => GuideType::Perspective,
        GuideKind::Symmetry => GuideType::Symmetry,
    }
}

fn length_unit(u: GuideUnit) -> LengthUnit {
    match u {
        GuideUnit::Millimeters => LengthUnit::Millimetres,
        GuideUnit::Centimeters => LengthUnit::Centimetres,
        GuideUnit::Inches => LengthUnit::Inches,
        GuideUnit::Pixels => LengthUnit::Pixels,
    }
}

fn guide_unit(u: LengthUnit) -> GuideUnit {
    match u {
        LengthUnit::Millimetres => GuideUnit::Millimeters,
        LengthUnit::Centimetres => GuideUnit::Centimeters,
        LengthUnit::Inches => GuideUnit::Inches,
        LengthUnit::Pixels => GuideUnit::Pixels,
    }
}

/// `px` canvas pixels in `unit` at `dpi`, as the grid size's keypad and pill show it: pixels
/// whole, the others to two decimals at most without trailing zeros ("71", "8", "0.31").
fn length_text(px: f32, unit: LengthUnit, dpi: f32) -> String {
    match unit {
        LengthUnit::Pixels => format!("{}", px.round() as i64),
        _ => crate::presets::preset_number(unit.from_pixels(px, dpi)),
    }
}

/// [`length_text`] and the unit's mark, without a space: "71px", "8mm", "10cm", "1″".
fn length_label(px: f32, unit: LengthUnit, dpi: f32) -> String {
    let mark = match unit {
        LengthUnit::Pixels => "px",
        LengthUnit::Millimetres => "mm",
        LengthUnit::Centimetres => "cm",
        LengthUnit::Inches => "\u{2033}",
    };
    format!("{}{mark}", length_text(px, unit, dpi))
}

/// `stamps` and the copies the guide's symmetry draws of them.
pub(crate) fn mirrored(guide: &Guide, stamps: &[Stamp]) -> Vec<Stamp> {
    let mut out = stamps.to_vec();
    for s in stamps {
        for (at, turn, mirror) in guide.copies(s.center) {
            let mut copy = *s;
            copy.center = at;
            if mirror {
                copy.angle = turn - s.angle;
                copy.flip = [!s.flip[0], s.flip[1]];
            } else {
                copy.angle = s.angle + turn;
            }
            out.push(copy);
        }
    }
    out
}

/// How far (window pixels) an assisted stroke goes before the direction it is kept to is settled.
const ASSIST_SETTLE: f32 = 10.0;

/// Vanishing points closer than this to the input (window pixels) are taken, not added.
const VANISHING_REACH: f32 = 30.0;

impl State {
    /// Stroke input kept to the guide when assisted drawing is on: along the guide direction
    /// the stroke heads in once it has gone [`ASSIST_SETTLE`] window pixels, kept to its end,
    /// so a wavering hand doesn't switch it (before then, held where it began).
    pub(crate) fn assisted(&mut self, mut samples: Vec<Sample>) -> Vec<Sample> {
        if let Some(start) = self.assist_from {
            let forced = self.force_assist.then(|| {
                let mut g = self.doc.guide.clone();
                g.assisted = true;
                g
            });
            let layer_guide = self.doc.stroke_guide();
            let guide = forced.as_ref().unwrap_or(&layer_guide);
            let settle = ASSIST_SETTLE / self.view.map_or(1.0, |v| v.zoom()).max(1e-6);
            for s in &mut samples {
                let heading = match self.assist_angle {
                    Some(a) => a,
                    None => {
                        let Some(a) = guide.assist_direction(start, s.pos) else { continue };
                        if s.pos.distance(start) < settle {
                            // too near the start to tell the direction yet: held there
                            s.pos = start;
                            continue;
                        }
                        self.assist_angle = Some(a);
                        a
                    }
                };
                s.pos = procraft_engine::guide::assist_along(start, s.pos, heading);
            }
        }
        samples
    }

    /// The canvas's dots per inch, the usual where it has none that can be used.
    fn dpi(&self) -> f32 {
        let dpi = self.doc.dpi;
        if dpi.is_finite() && dpi > 0.0 { dpi } else { procraft_engine::formats::DPI as f32 }
    }

    fn guide_info(&self) -> GuideInfo {
        // radial symmetry's lines as a plugin has them, if one says
        let mut seen = self.doc.guide.clone();
        seen.radial = self.hooks.as_ref().and_then(|hooks| hooks.radial_segments());
        let g = &seen;
        let look = g.look();
        let dpi = self.dpi();
        let to_screen = self.view.map_or(Affine::scale(1.0), |v| v.to_screen());
        let point = |p: Vec2| {
            let w = to_screen.apply(p);
            [w.x, w.y]
        };
        let c = Vec2::new(g.centre[0], g.centre[1]);
        // the turn handle sits up from the centre, at the canvas's top edge when level
        let up = Vec2::new(g.angle.sin(), -g.angle.cos()) * c.y.max(1.0);
        GuideInfo {
            on: g.on,
            kind: type_of(g.kind),
            opacity: look.opacity,
            thickness: look.thickness,
            size: look.size,
            unit: guide_unit(look.unit),
            size_text: length_text(look.size, look.unit, dpi),
            size_label: length_label(look.size, look.unit, dpi),
            hue: look.color,
            color: bar_color(look.color).to_vec(),
            symmetry: match g.symmetry {
                SymmetryKind::Vertical => SymmetryType::Vertical,
                SymmetryKind::Horizontal => SymmetryType::Horizontal,
                SymmetryKind::Quadrant => SymmetryType::Quadrant,
                SymmetryKind::Radial => SymmetryType::Radial,
            },
            rotational: g.rotational,
            assisted: self.doc.is_assisted(self.doc.current()),
            centre: point(c).to_vec(),
            handle: point(c + up).to_vec(),
            vanishing: g.vanishing.iter().flat_map(|v| point(Vec2::new(v[0], v[1]))).collect(),
            lines: g
                .lines(self.doc.width(), self.doc.height())
                .into_iter()
                .flat_map(|(a, b)| point(a).into_iter().chain(point(b)))
                .collect(),
        }
    }

    fn to_canvas_point(&self, x: f32, y: f32) -> Option<Vec2> {
        self.view?.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite())
    }
}

#[uniffi::export]
impl CanvasSession {
    pub fn guide(&self) -> GuideInfo {
        self.shared.lock().guide_info()
    }

    pub fn set_guide_on(&self, on: bool) -> GuideInfo {
        self.change_guide(|g| g.on = on)
    }

    pub fn set_guide_kind(&self, kind: GuideType) -> GuideInfo {
        self.change_guide(|g| g.kind = kind_of(kind))
    }

    /// The shown kind's opacity and thickness (0..1), grid size (canvas pixels) and colour (0..1
    /// along the colour bar).
    pub fn set_guide_look(&self, opacity: f32, thickness: f32, size: f32, hue: f32) -> GuideInfo {
        self.change_guide(|g| {
            let k = g.kind as usize;
            if let Some(look) = g.looks.get_mut(k) {
                let unit = |v: f32, old: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { old };
                look.opacity = unit(opacity, look.opacity);
                look.thickness = unit(thickness, look.thickness);
                look.color = unit(hue, look.color);
                if size.is_finite() {
                    look.size = size.clamp(SIZE_MIN, SIZE_MAX);
                }
            }
        })
    }

    /// The shown grid's size typed on its keypad: `typed` (what is typed so far) in `unit`, made
    /// canvas pixels by the canvas's DPI and kept to the sizes a grid can have. The unit is kept
    /// for the size's pill even while nothing (or no number above nothing) is typed.
    pub fn set_guide_length(&self, typed: String, unit: GuideUnit) -> GuideInfo {
        let dpi = self.shared.lock().dpi();
        let unit = length_unit(unit);
        let px = typed.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|v| unit.to_pixels(v, dpi));
        self.change_guide(|g| {
            let k = g.kind as usize;
            if let Some(look) = g.looks.get_mut(k) {
                look.unit = unit;
                if let Some(px) = px.filter(|px| px.is_finite()) {
                    look.size = px.clamp(SIZE_MIN, SIZE_MAX);
                }
            }
        })
    }

    pub fn set_guide_symmetry(&self, kind: SymmetryType, rotational: bool) -> GuideInfo {
        self.change_guide(|g| {
            g.symmetry = match kind {
                SymmetryType::Vertical => SymmetryKind::Vertical,
                SymmetryType::Horizontal => SymmetryKind::Horizontal,
                SymmetryType::Quadrant => SymmetryKind::Quadrant,
                SymmetryType::Radial => SymmetryKind::Radial,
            };
            g.rotational = rotational;
        })
    }

    /// Strokes from now on follow the drawing guide (if it is on) even where drawing assist is
    /// off, until this is turned off again: □ + Touch or □ + Pen set to assisted drawing.
    pub fn set_stroke_assist(&self, on: bool) {
        self.shared.lock().force_assist = on;
    }

    /// Drawing assist on the current layer (a mask's layer, for a mask): strokes on it follow the
    /// guide. One step, named as measured ("Undo Drawing Assist on").
    pub fn set_guide_assisted(&self, assisted: bool) -> GuideInfo {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        let owner = match st.doc.layer(current) {
            Some(l) if l.kind == procraft_engine::document::LayerKind::Mask => l.parent.unwrap_or(current),
            _ => current,
        };
        if st.doc.layer(owner).is_some_and(|l| l.assisted != assisted) {
            st.finish_stroke();
            st.settle();
            let before = st.doc.clone();
            st.doc.set_assisted(owner, assisted);
            if let Some(canvas) = st.canvas.as_mut() {
                canvas.name_next_step(if assisted { "Drawing Assist on" } else { "Drawing Assist off" });
                canvas.record_document(before);
            }
        }
        st.revision += 1;
        self.shared.frame(st);
        st.guide_info()
    }

    /// The guide's centre dragged to window point (`x`, `y`).
    pub fn move_guide_centre(&self, x: f32, y: f32) -> GuideInfo {
        let to = self.shared.lock().to_canvas_point(x, y);
        self.change_guide(|g| {
            if let Some(p) = to {
                g.centre = [p.x, p.y];
            }
        })
    }

    /// The guide's turn handle dragged to window point (`x`, `y`): it turns about its centre.
    pub fn turn_guide(&self, x: f32, y: f32) -> GuideInfo {
        let to = self.shared.lock().to_canvas_point(x, y);
        self.change_guide(|g| {
            if let Some(p) = to {
                let d = p - Vec2::new(g.centre[0], g.centre[1]);
                if d.length() > 1.0 {
                    g.angle = d.x.atan2(-d.y);
                }
            }
        })
    }

    /// Perspective: a tap at window point (`x`, `y`) adds a vanishing point (up to three); returns
    /// the index of the point there, to drag.
    pub fn touch_vanishing_point(&self, x: f32, y: f32) -> Option<u32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let p = st.to_canvas_point(x, y)?;
        let to_screen = st.view?.to_screen();
        let near = st
            .doc
            .guide
            .vanishing
            .iter()
            .position(|v| to_screen.apply(Vec2::new(v[0], v[1])).distance(Vec2::new(x, y)) <= VANISHING_REACH);
        if let Some(i) = near {
            return Some(i as u32);
        }
        if st.doc.guide.vanishing.len() >= 3 {
            return None;
        }
        st.doc.guide.vanishing.push([p.x, p.y]);
        st.revision += 1;
        self.shared.frame(st);
        Some(st.doc.guide.vanishing.len() as u32 - 1)
    }

    pub fn move_vanishing_point(&self, index: u32, x: f32, y: f32) -> GuideInfo {
        let to = self.shared.lock().to_canvas_point(x, y);
        self.change_guide(|g| {
            if let (Some(p), Some(v)) = (to, g.vanishing.get_mut(index as usize)) {
                *v = [p.x, p.y];
            }
        })
    }

    pub fn remove_vanishing_point(&self, index: u32) -> GuideInfo {
        self.change_guide(|g| {
            if (index as usize) < g.vanishing.len() {
                g.vanishing.remove(index as usize);
            }
        })
    }

    /// Opens the guide editor: the guide as it was is kept to go back to, and the canvas fits
    /// inside the window rectangle `left`..`right` × `top`..`bottom`.
    pub fn guide_edit_begin(&self, left: f32, top: f32, right: f32, bottom: f32) -> GuideInfo {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.guide_before = Some((st.doc.guide.clone(), st.view));
        if st.doc.guide.centre == [0.0, 0.0] {
            st.doc.guide.centre = [st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0];
        }
        let size = ((right - left).max(1.0) as u32, (bottom - top).max(1.0) as u32);
        let fitted = View::fit((st.doc.width(), st.doc.height()), size);
        st.view = Some(View::from_affine(fitted.to_screen().then(Affine::translate(Vec2::new(left, top)))));
        self.shared.frame(st);
        st.guide_info()
    }

    /// Closes the guide editor: `keep` keeps the changes, otherwise the guide goes back. The view
    /// goes back as it was either way.
    pub fn guide_edit_end(&self, keep: bool) -> GuideInfo {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some((guide, view)) = st.guide_before.take() {
            if !keep {
                st.doc.guide = guide;
            }
            st.view = view;
        }
        st.revision += 1;
        self.shared.frame(st);
        st.guide_info()
    }
}

impl CanvasSession {
    fn change_guide(&self, f: impl FnOnce(&mut Guide)) -> GuideInfo {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        f(&mut st.doc.guide);
        // once on, it stays set up
        st.doc.guide.set_up |= st.doc.guide.on;
        st.revision += 1;
        self.shared.frame(st);
        st.guide_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_size_shows_in_its_unit_whole_or_to_two_decimals() {
        // whole pixels; the lengths as measured, a whole number without decimals
        assert_eq!(length_label(71.0, LengthUnit::Pixels, 132.0), "71px");
        assert_eq!(length_label(71.4, LengthUnit::Pixels, 132.0), "71px");
        assert_eq!(length_label(LengthUnit::Millimetres.to_pixels(8.0, 132.0), LengthUnit::Millimetres, 132.0), "8mm");
        assert_eq!(
            length_label(LengthUnit::Centimetres.to_pixels(10.0, 132.0), LengthUnit::Centimetres, 132.0),
            "10cm"
        );
        assert_eq!(length_label(300.0, LengthUnit::Inches, 300.0), "1\u{2033}");
        // two decimals at most, no trailing zeros
        assert_eq!(length_text(150.0, LengthUnit::Inches, 300.0), "0.5");
        assert_eq!(length_text(71.0, LengthUnit::Millimetres, 132.0), "13.66");
        assert_eq!(length_text(71.0, LengthUnit::Inches, 132.0), "0.54");
    }

    #[test]
    fn a_typed_length_becomes_the_grid_size_by_the_dpi_and_cancel_undoes_it() {
        let s = CanvasSession::new(400, 300);
        s.shared.lock().doc.dpi = 132.0;
        s.set_guide_on(true);
        let before = s.guide_edit_begin(0.0, 0.0, 400.0, 300.0);
        assert_eq!(
            (before.unit, before.size_text.as_str(), before.size_label.as_str()),
            (GuideUnit::Pixels, "71", "71px")
        );
        // a unit chosen with nothing typed keeps the size, shown in the unit
        let g = s.set_guide_length(String::new(), GuideUnit::Millimeters);
        assert_eq!((g.size, g.size_label.as_str()), (71.0, "13.66mm"));
        // typed: the size in canvas pixels, the pill in the unit
        let g = s.set_guide_length("8".into(), GuideUnit::Millimeters);
        assert!((g.size - 41.575).abs() < 1e-3 && g.size_label == "8mm" && g.size_text == "8", "{g:?}");
        let g = s.set_guide_length("10".into(), GuideUnit::Centimeters);
        assert!((g.size - 519.685).abs() < 1e-2 && g.size_label == "10cm", "{g:?}");
        assert_eq!(s.set_guide_length("1".into(), GuideUnit::Inches).size_label, "1\u{2033}");
        // what isn't a length above nothing leaves the size as it was
        for typed in [".", "0", "0.", "1.2.3"] {
            assert_eq!(s.set_guide_length(typed.into(), GuideUnit::Inches).size, 132.0, "{typed}");
        }
        assert_eq!(s.set_guide_length("1.".into(), GuideUnit::Inches).size, 132.0);
        assert_eq!(s.set_guide_length("0.5".into(), GuideUnit::Inches).size_label, "0.5\u{2033}");
        // kept to the sizes a grid can have, shown as kept
        let g = s.set_guide_length("100".into(), GuideUnit::Centimeters);
        assert_eq!((g.size, g.size_label.as_str()), (SIZE_MAX, "19.24cm"));
        let g = s.set_guide_length("1".into(), GuideUnit::Pixels);
        assert_eq!((g.size, g.size_label.as_str()), (SIZE_MIN, "4px"));
        // the slider keeps the unit
        assert_eq!(s.set_guide_look(g.opacity, g.thickness, 264.0, g.hue).size_label, "264px");
        s.set_guide_length("2".into(), GuideUnit::Inches);
        assert_eq!(s.set_guide_look(g.opacity, g.thickness, 66.0, g.hue).size_label, "0.5\u{2033}");
        // each kind keeps its own; Cancel puts size and unit back
        let iso = s.set_guide_kind(GuideType::Isometric);
        assert_eq!((iso.size, iso.unit), (82.0, GuideUnit::Pixels));
        s.set_guide_kind(GuideType::Grid);
        let back = s.guide_edit_end(false);
        assert_eq!((back.size, back.unit, back.size_label.as_str()), (71.0, GuideUnit::Pixels, "71px"));
        // Done keeps them
        s.guide_edit_begin(0.0, 0.0, 400.0, 300.0);
        s.set_guide_length("3".into(), GuideUnit::Centimeters);
        assert_eq!(s.guide_edit_end(true).size_label, "3cm");
    }
}
