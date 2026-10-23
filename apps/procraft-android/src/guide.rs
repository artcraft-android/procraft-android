//! The drawing guide: set from the guide editor, drawn over the canvas as lines, and followed by
//! strokes with assisted drawing on (kept to the guide's directions, or mirrored by symmetry).

use procraft_engine::geom::{Affine, Vec2};
use procraft_engine::guide::{Guide, GuideKind, SymmetryKind, bar_color};
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

/// The guide as the editor and the overlay show it. Points are window pixels, x and y in turn;
/// `lines` are segments (two points each); `color` is sRGB 0..1.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GuideInfo {
    pub on: bool,
    pub kind: GuideType,
    pub opacity: f32,
    pub thickness: f32,
    /// The grid's size, canvas pixels.
    pub size: f32,
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

/// Vanishing points closer than this to the input (window pixels) are taken, not added.
const VANISHING_REACH: f32 = 30.0;

impl State {
    /// Stroke input kept to the guide when assisted drawing is on.
    pub(crate) fn assisted(&self, mut samples: Vec<Sample>) -> Vec<Sample> {
        if let Some(start) = self.assist_from {
            let forced = self.force_assist.then(|| {
                let mut g = self.doc.guide.clone();
                g.assisted = true;
                g
            });
            let guide = forced.as_ref().unwrap_or(&self.doc.guide);
            for s in &mut samples {
                s.pos = guide.assist(start, s.pos);
            }
        }
        samples
    }

    fn guide_info(&self) -> GuideInfo {
        let g = &self.doc.guide;
        let look = g.look();
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
            hue: look.color,
            color: bar_color(look.color).to_vec(),
            symmetry: match g.symmetry {
                SymmetryKind::Vertical => SymmetryType::Vertical,
                SymmetryKind::Horizontal => SymmetryType::Horizontal,
                SymmetryKind::Quadrant => SymmetryType::Quadrant,
                SymmetryKind::Radial => SymmetryType::Radial,
            },
            rotational: g.rotational,
            assisted: g.assisted,
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
                    look.size = size.clamp(4.0, 1000.0);
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

    pub fn set_guide_assisted(&self, assisted: bool) -> GuideInfo {
        self.change_guide(|g| g.assisted = assisted)
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
        st.revision += 1;
        self.shared.frame(st);
        st.guide_info()
    }
}
