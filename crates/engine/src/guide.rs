//! Drawing guides over the canvas: a 2D grid, an isometric grid, perspective, or symmetry, drawn as
//! lines to follow. With assisted drawing on, strokes keep to them: along the grid's directions,
//! towards the vanishing points, or mirrored about the symmetry.

use std::f32::consts::{FRAC_PI_6, PI, TAU};

use crate::geom::Vec2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GuideKind {
    #[default]
    Grid,
    Isometric,
    Perspective,
    Symmetry,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymmetryKind {
    #[default]
    Vertical,
    Horizontal,
    Quadrant,
    Radial,
}

/// One kind of guide's look: its opacity and line thickness (0..1), its grid's size (canvas
/// pixels), and its colour (0..1 along the colour bar, see [`bar_color`]).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GuideLook {
    pub opacity: f32,
    pub thickness: f32,
    pub size: f32,
    pub color: f32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Guide {
    pub on: bool,
    pub kind: GuideKind,
    /// Each kind's look, in [`GuideKind`] order.
    pub looks: [GuideLook; 4],
    /// Where the grid starts and the symmetry turns about (canvas pixels), and its turn (radians).
    pub centre: [f32; 2],
    pub angle: f32,
    pub symmetry: SymmetryKind,
    /// Symmetry turns copies round instead of mirroring them.
    pub rotational: bool,
    pub assisted: bool,
    /// Perspective's vanishing points (canvas pixels), one to three.
    pub vanishing: Vec<[f32; 2]>,
    /// The guide has been on at some time: from then on the layers' menus offer Drawing Assist,
    /// on or off.
    #[serde(default)]
    pub set_up: bool,
}

/// The kinds' looks as a new guide starts.
pub const STARTS: [GuideLook; 4] = [
    GuideLook { opacity: 0.36, thickness: 0.64, size: 71.0, color: 0.0 },
    GuideLook { opacity: 0.30, thickness: 0.60, size: 82.0, color: 0.0 },
    GuideLook { opacity: 1.0, thickness: 0.70, size: 71.0, color: 0.33 },
    GuideLook { opacity: 0.41, thickness: 0.60, size: 71.0, color: 0.0 },
];

/// The guide colour bar: dark through blue, green, orange, red and pink to white, as stops.
const BAR: [[u8; 3]; 21] = [
    [5, 4, 10],
    [1, 12, 44],
    [0, 24, 87],
    [8, 57, 132],
    [18, 139, 182],
    [25, 198, 203],
    [27, 208, 184],
    [47, 208, 131],
    [103, 192, 100],
    [188, 155, 76],
    [206, 144, 73],
    [217, 118, 68],
    [218, 87, 74],
    [220, 65, 77],
    [229, 61, 113],
    [236, 54, 171],
    [245, 52, 222],
    [245, 92, 249],
    [251, 146, 253],
    [252, 202, 255],
    [232, 225, 233],
];

/// The colour (sRGB 0..1) at `t` (0..1) along the guide colour bar.
pub fn bar_color(t: f32) -> [f32; 3] {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let at = t * (BAR.len() - 1) as f32;
    let i = (at.floor() as usize).min(BAR.len() - 2);
    let f = at - i as f32;
    let (Some(a), Some(b)) = (BAR.get(i), BAR.get(i + 1)) else { return [0.0; 3] };
    [0, 1, 2].map(|k| {
        let (x, y) = (f32::from(a.get(k).copied().unwrap_or(0)), f32::from(b.get(k).copied().unwrap_or(0)));
        (x + (y - x) * f) / 255.0
    })
}

/// The place (0..1) along the guide colour bar whose colour is nearest `rgb` (sRGB 0..1).
pub fn bar_place(rgb: [f32; 3]) -> f32 {
    const STEPS: u32 = 1000;
    let far = |t: f32| bar_color(t).iter().zip(rgb).map(|(a, b)| (a - b).powi(2)).sum::<f32>();
    (0..=STEPS).map(|i| i as f32 / STEPS as f32).min_by(|a, b| far(*a).total_cmp(&far(*b))).unwrap_or(0.0)
}

/// Rays a perspective guide's vanishing point sends out each way from where its fan starts.
const RAYS_EACH_WAY: usize = 24;

/// Vanishing point `i`'s rays over a `w` × `h` canvas, fanned as the reference fans them: from the
/// way towards the other point of the horizon (a third point, or a lone one, towards the canvas's
/// middle), 24 either way. From a point on the canvas they go all the way round, 7.5° apart; from one off it,
/// they spread over the angle the canvas fills as seen from there, a 24th of it apart (those
/// that miss the canvas are left out by the clip). Each is long enough to cross the canvas.
fn perspective_rays(points: &[Vec2], i: usize, w: f32, h: f32) -> Vec<(Vec2, Vec2)> {
    let Some(&v) = points.get(i) else { return Vec::new() };
    let mid = Vec2::new(w / 2.0, h / 2.0);
    let other = match i {
        0 => points.get(1).copied(),
        1 => points.first().copied(),
        _ => None,
    };
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)];
    let angle = |d: Vec2| d.y.atan2(d.x);
    let inside = (0.0..=w).contains(&v.x) && (0.0..=h).contains(&v.y);
    let (start, step) = if inside {
        // a lone or third point on the canvas starts level, as a guide's horizon does by default
        (other.map_or(0.0, |o| angle(o - v)), TAU / (2 * RAYS_EACH_WAY) as f32)
    } else {
        // the angle the canvas fills, its corners measured about the way to its middle
        let towards = angle(mid - v);
        let about = corners.map(|k| (angle(k - v) - towards + PI).rem_euclid(TAU) - PI);
        let fills = about.iter().copied().fold(f32::MIN, f32::max) - about.iter().copied().fold(f32::MAX, f32::min);
        (angle(other.unwrap_or(mid) - v), fills / RAYS_EACH_WAY as f32)
    };
    // a canvas seen edge on (or a step that is no number) has no fan
    if step.is_nan() || step <= 0.0 {
        return Vec::new();
    }
    let length = corners.iter().map(|k| v.distance(*k)).fold(0.0, f32::max) + 1.0;
    (0..RAYS_EACH_WAY)
        .flat_map(|k| [start + step * k as f32, start - step * (k + 1) as f32])
        .map(|a| (v, v + Vec2::new(a.cos(), a.sin()) * length))
        .collect()
}

/// The part of the segment `a`–`b` inside `0..w` × `0..h` (Liang–Barsky), if a visible part is.
fn clip(a: Vec2, b: Vec2, w: f32, h: f32) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x), (d.x, w - a.x), (-d.y, a.y), (d.y, h - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    let (s, e) = (a + d * t0, a + d * t1);
    (t0 < t1 && s.distance(e) >= 0.5).then_some((s, e))
}

impl Default for Guide {
    /// A guide for no canvas in particular (documents saved before guides; recentred on opening).
    fn default() -> Guide {
        Guide::new(0, 0)
    }
}

impl Guide {
    /// A guide for a `width` × `height` canvas, off, centred on it.
    pub fn new(width: u32, height: u32) -> Guide {
        Guide {
            on: false,
            kind: GuideKind::Grid,
            looks: STARTS,
            centre: [width as f32 / 2.0, height as f32 / 2.0],
            angle: 0.0,
            symmetry: SymmetryKind::Vertical,
            rotational: false,
            assisted: false,
            vanishing: Vec::new(),
            set_up: false,
        }
    }

    pub fn look(&self) -> GuideLook {
        self.looks.get(self.kind as usize).copied().unwrap_or(STARTS[0])
    }

    fn centre(&self) -> Vec2 {
        Vec2::new(self.centre[0], self.centre[1])
    }

    /// The guide's lines over a `width` × `height` canvas, as segments (canvas pixels) kept within
    /// it.
    pub fn lines(&self, width: u32, height: u32) -> Vec<(Vec2, Vec2)> {
        let (w, h) = (width as f32, height as f32);
        self.unclipped(width, height).into_iter().filter_map(|(a, b)| clip(a, b, w, h)).collect()
    }

    fn unclipped(&self, width: u32, height: u32) -> Vec<(Vec2, Vec2)> {
        let (w, h) = (width as f32, height as f32);
        let reach = w.hypot(h);
        let c = self.centre();
        let size = self.look().size.max(4.0);
        let family = |dir: f32| -> Vec<(Vec2, Vec2)> {
            // parallel lines `size` apart through the centre, long enough to cross the canvas
            let along = Vec2::new(dir.cos(), dir.sin());
            let across = Vec2::new(-along.y, along.x);
            let n = (reach / size).ceil() as i32 + 1;
            (-n..=n)
                .map(|k| {
                    let o = c + across * (k as f32 * size);
                    (o - along * reach, o + along * reach)
                })
                .collect()
        };
        match self.kind {
            GuideKind::Grid => [family(self.angle), family(self.angle + PI / 2.0)].concat(),
            GuideKind::Isometric => {
                // verticals, and lines thirty degrees either side of the horizontal, spaced so
                // the three meet in triangles
                let iso = size * FRAC_PI_6.cos();
                let mut out = Vec::new();
                for dir in [self.angle + PI / 2.0, self.angle + FRAC_PI_6, self.angle - FRAC_PI_6] {
                    let along = Vec2::new(dir.cos(), dir.sin());
                    let across = Vec2::new(-along.y, along.x);
                    let n = (reach / iso).ceil() as i32 + 1;
                    out.extend((-n..=n).map(|k| {
                        let o = c + across * (k as f32 * iso);
                        (o - along * reach, o + along * reach)
                    }));
                }
                out
            }
            GuideKind::Perspective => {
                // each vanishing point's rays, and the horizon through the first two (a lone
                // point's level through it)
                let points: Vec<Vec2> = self.vanishing.iter().map(|v| Vec2::new(v[0], v[1])).collect();
                let mut out = Vec::new();
                for i in 0..points.len() {
                    out.extend(perspective_rays(&points, i, w, h));
                }
                match points.as_slice() {
                    [a, b, ..] => {
                        let d = (*b - *a).normalized_or(Vec2::new(1.0, 0.0));
                        let far = reach + a.distance(c) + b.distance(c);
                        out.push((*a - d * far, *b + d * far));
                    }
                    [a] => {
                        let far = reach + a.distance(c);
                        out.push((*a - Vec2::new(far, 0.0), *a + Vec2::new(far, 0.0)));
                    }
                    [] => {}
                }
                out
            }
            GuideKind::Symmetry => {
                let axes: Vec<f32> = match self.symmetry {
                    SymmetryKind::Vertical => vec![self.angle + PI / 2.0],
                    SymmetryKind::Horizontal => vec![self.angle],
                    SymmetryKind::Quadrant => vec![self.angle, self.angle + PI / 2.0],
                    SymmetryKind::Radial => (0..4).map(|k| self.angle + k as f32 * PI / 4.0).collect(),
                };
                axes.into_iter()
                    .map(|a| {
                        let d = Vec2::new(a.cos(), a.sin()) * reach;
                        (c - d, c + d)
                    })
                    .collect()
            }
        }
    }

    /// Where a stamp at `p` is drawn again by the symmetry (not counting `p` itself), each with the
    /// turn to add to its angle and whether it is mirrored.
    pub fn copies(&self, p: Vec2) -> Vec<(Vec2, f32, bool)> {
        if !self.on || !self.assisted || self.kind != GuideKind::Symmetry {
            return Vec::new();
        }
        let c = self.centre();
        let turn = |q: Vec2, a: f32| {
            let d = q - c;
            c + Vec2::new(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos())
        };
        // mirrored across the line through the centre at angle `a`
        let mirror = |q: Vec2, a: f32| {
            let d = q - c;
            let (s, co) = ((2.0 * a).sin(), (2.0 * a).cos());
            c + Vec2::new(d.x * co + d.y * s, d.x * s - d.y * co)
        };
        let base = self.angle;
        let (count, axis) = match self.symmetry {
            SymmetryKind::Vertical => (2, base + PI / 2.0),
            SymmetryKind::Horizontal => (2, base),
            SymmetryKind::Quadrant => (4, base),
            SymmetryKind::Radial => (8, base),
        };
        if self.rotational || count > 2 {
            // turned copies; mirrored ones in between for a kaleidoscope unless only turning
            let step = TAU / count as f32;
            let mut out = Vec::new();
            for k in 1..count {
                let a = step * k as f32;
                if self.rotational || k % 2 == 0 {
                    out.push((turn(p, a), a, false));
                } else {
                    // the mirror axis between the two neighbouring turned copies
                    let m = axis + a / 2.0;
                    out.push((mirror(p, m), 2.0 * m, true));
                }
            }
            return out;
        }
        vec![(mirror(p, axis), 2.0 * axis, true)]
    }

    /// Assisted drawing: where a stroke begun at `start` goes when the input is at `p` (canvas
    /// pixels), kept along the guide's nearest direction.
    pub fn assist(&self, start: Vec2, p: Vec2) -> Vec2 {
        match self.assist_direction(start, p) {
            Some(a) => assist_along(start, p, a),
            None => p,
        }
    }

    /// The direction (an angle, radians) a stroke from `start` that has reached `p` is kept to:
    /// of the guide's directions, the one its movement is nearest to; none when the guide doesn't
    /// assist (off, not assisted, or symmetry).
    pub fn assist_direction(&self, start: Vec2, p: Vec2) -> Option<f32> {
        if !self.on || !self.assisted {
            return None;
        }
        let d = p - start;
        let along = |dirs: &[f32]| -> f32 {
            // the direction (or its reverse) the movement is nearest to
            dirs.iter()
                .copied()
                .fold((0.0f32, f32::MIN), |best, a| {
                    let dot = d.dot(Vec2::new(a.cos(), a.sin())).abs();
                    if dot > best.1 { (a, dot) } else { best }
                })
                .0
        };
        Some(match self.kind {
            GuideKind::Grid => along(&[self.angle, self.angle + PI / 2.0]),
            GuideKind::Isometric => along(&[self.angle + PI / 2.0, self.angle + FRAC_PI_6, self.angle - FRAC_PI_6]),
            GuideKind::Perspective => {
                // towards one of the vanishing points, or level or upright
                let mut dirs: Vec<f32> = self
                    .vanishing
                    .iter()
                    .map(|v| {
                        let to = Vec2::new(v[0], v[1]) - start;
                        to.y.atan2(to.x)
                    })
                    .collect();
                if self.vanishing.len() < 3 {
                    dirs.push(PI / 2.0);
                }
                if self.vanishing.len() < 2 {
                    dirs.push(0.0);
                }
                along(&dirs)
            }
            GuideKind::Symmetry => return None,
        })
    }
}

/// `p` brought onto the line through `start` at angle `a` (radians).
pub fn assist_along(start: Vec2, p: Vec2, a: f32) -> Vec2 {
    let u = Vec2::new(a.cos(), a.sin());
    start + u * (p - start).dot(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(kind: GuideKind) -> Guide {
        Guide { on: true, kind, assisted: true, ..Guide::new(200, 100) }
    }

    #[test]
    fn grids_cross_the_canvas_in_their_spacing() {
        let g = on(GuideKind::Grid);
        let lines = g.lines(200, 100);
        // vertical lines 71 apart through the centre: x = 100 ± 71k
        assert!(lines.iter().any(|(a, b)| (a.x - 171.0).abs() < 1e-3 && (b.x - 171.0).abs() < 1e-3));
        assert!(!lines.is_empty());
        assert!(!on(GuideKind::Isometric).lines(200, 100).is_empty());
        let mut p = on(GuideKind::Perspective);
        assert!(p.lines(200, 100).is_empty());
        p.vanishing = vec![[0.0, 50.0], [200.0, 50.0]];
        let rays = p.lines(200, 100);
        assert!(rays.len() > 30);
        // a lone point keeps a level horizon through it
        p.vanishing = vec![[100.0, 50.0]];
        assert!(
            p.lines(200, 100)
                .iter()
                .any(|(a, b)| (a.y - 50.0).abs() < 1e-3 && (b.y - 50.0).abs() < 1e-3 && (b.x - a.x).abs() > 199.0)
        );
        p.vanishing = vec![[0.0, 50.0], [200.0, 50.0]];
        assert!(
            rays.iter()
                .all(|(a, b)| [a, b].iter().all(|q| q.x >= -1e-3 && q.x <= 200.001 && q.y >= -1e-3 && q.y <= 100.001))
        );
    }

    #[test]
    fn perspective_rays_fan_24_each_way_and_cross_the_canvas() {
        let (w, h) = (3840.0, 2160.0);
        let angle = |(a, b): &(Vec2, Vec2)| (b.y - a.y).atan2(b.x - a.x);
        // on the canvas: 48 rays 7.5° apart, one of them along the horizon to the other point
        let on_canvas = [Vec2::new(1000.0, 1000.0), Vec2::new(3000.0, 1000.0)];
        let rays = perspective_rays(&on_canvas, 0, w, h);
        assert_eq!(rays.len(), 48);
        assert!(rays.iter().any(|r| angle(r).abs() < 1e-5));
        let mut a: Vec<f32> = rays.iter().map(|r| angle(r).rem_euclid(TAU)).collect();
        a.sort_by(f32::total_cmp);
        assert!(a.windows(2).all(|p| (p[1] - p[0] - TAU / 48.0).abs() < 1e-4));
        // far off it, even far away: the rays still cross the canvas, a 24th of the angle it fills
        // apart, fanned from the way to the other point
        let far = [Vec2::new(-2191.25, 1636.2), Vec2::new(4522.7, 2380.2), Vec2::new(2744.9, -8519.9)];
        for i in 0..3 {
            let rays = perspective_rays(&far, i, w, h);
            assert_eq!(rays.len(), 48);
            let crossing = rays.iter().filter_map(|(a, b)| clip(*a, *b, w, h)).count();
            assert!(crossing >= 12, "point {i}: {crossing} cross the canvas");
        }
        let g = Guide { vanishing: far.iter().map(|v| [v.x, v.y]).collect(), ..on(GuideKind::Perspective) };
        assert!(g.lines(3840, 2160).len() > 40);
    }

    #[test]
    fn assisted_strokes_keep_to_the_guide() {
        let g = on(GuideKind::Grid);
        let start = Vec2::new(10.0, 10.0);
        assert_eq!(g.assist(start, Vec2::new(50.0, 14.0)), Vec2::new(50.0, 10.0));
        assert!((g.assist(start, Vec2::new(12.0, 60.0)).x - 10.0).abs() < 1e-4);
        let iso = on(GuideKind::Isometric);
        let p = iso.assist(start, Vec2::new(60.0, 40.0));
        let d = p - start;
        assert!((d.y.atan2(d.x) - FRAC_PI_6).abs() < 1e-4);
        // off, nothing moves
        let off = Guide { assisted: false, ..g };
        assert_eq!(off.assist(start, Vec2::new(50.0, 14.0)), Vec2::new(50.0, 14.0));
    }

    #[test]
    fn symmetry_mirrors_and_turns() {
        let g = on(GuideKind::Symmetry);
        // vertical: mirrored across x = 100
        let c = g.copies(Vec2::new(70.0, 20.0));
        assert_eq!(c.len(), 1);
        assert!((c[0].0.x - 130.0).abs() < 1e-3 && (c[0].0.y - 20.0).abs() < 1e-3 && c[0].2);
        let quad = Guide { symmetry: SymmetryKind::Quadrant, ..g.clone() };
        assert_eq!(quad.copies(Vec2::new(70.0, 20.0)).len(), 3);
        let radial = Guide { symmetry: SymmetryKind::Radial, rotational: true, ..g.clone() };
        let r = radial.copies(Vec2::new(110.0, 50.0));
        assert_eq!(r.len(), 7);
        assert!(r.iter().all(|(q, _, mirrored)| (q.distance(Vec2::new(100.0, 50.0)) - 10.0).abs() < 1e-3 && !mirrored));
        assert!(on(GuideKind::Grid).copies(Vec2::ZERO).is_empty());
    }

    #[test]
    fn the_colour_bar_runs_dark_to_light() {
        assert!(bar_color(0.0).iter().all(|&v| v < 0.05));
        assert!(bar_color(1.0).iter().all(|&v| v > 0.85));
        assert_eq!(bar_color(f32::NAN), bar_color(0.0));
    }
}
