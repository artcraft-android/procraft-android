//! Crop and Resize as it is edited, the same in every app: the rectangle kept (canvas pixels), its
//! turn, the size it becomes (resampled, or the rectangle's), the DPI, and the rules a drag on the
//! frame follows: which part a press takes, snapping to the canvas's sides and middle, a locked
//! aspect following the width. The app draws the frame and the settings; Done crops (crop.rs).

use procraft_engine::document::canvas_edge_most;
use procraft_engine::geom::Vec2;

use crate::CanvasSession;

/// The canvas fitted to the screen and shrunk this much while cropping, so the frame's handles are
/// clear of the edges.
const CROP_SCALE: f32 = 0.597;
/// How near (points) a corner or a side's middle a press takes it, and how near (points) the frame
/// snaps.
const REACH: f32 = 28.0;
const SNAP: f32 = 10.0;

/// Which part of the frame a drag takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    Move,
    Left,
    Top,
    Right,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The crop as its frame and settings show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CropShown {
    /// The rectangle kept, canvas pixels.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Its turn, radians.
    pub angle: f32,
    pub resample: bool,
    /// The size what is kept becomes, pixels.
    pub size_width: u32,
    pub size_height: u32,
    pub aspect_locked: bool,
    pub snapping: bool,
    pub dpi: f32,
    /// The frame's corners in the window (x, y each: top left, top right, bottom right, bottom
    /// left), turned with the crop.
    pub corners: Vec<f32>,
}

#[derive(Default)]
pub(crate) struct CropEdit {
    /// Left, top, right, bottom; none outside Crop and Resize.
    rect: Option<[f32; 4]>,
    angle: f32,
    resample: bool,
    size: (u32, u32),
    aspect_locked: bool,
    snapping: bool,
    dpi: f32,
    /// A drag on the frame: what it took, the rectangle then, where it began (canvas pixels) and
    /// how far (canvas pixels) the frame snaps.
    drag: Option<(Grab, [f32; 4], Vec2, f32)>,
    /// Why the last crop couldn't be made, until the app takes it to tell, and whether the GPU's
    /// device being lost was told.
    pub(crate) failure: Option<String>,
    pub(crate) lost_told: bool,
}

impl CropEdit {
    /// The frame's corners in canvas pixels, turned with the crop.
    fn corners(&self, [l, t, r, b]: [f32; 4]) -> [Vec2; 4] {
        let c = Vec2::new((l + r) / 2.0, (t + b) / 2.0);
        let (s, k) = self.angle.sin_cos();
        [Vec2::new(l, t), Vec2::new(r, t), Vec2::new(r, b), Vec2::new(l, b)].map(|p| {
            let d = p - c;
            Vec2::new(c.x + d.x * k - d.y * s, c.y + d.x * s + d.y * k)
        })
    }

    /// Where the frame goes when the drag reaches `at` (canvas pixels) on a canvas `canvas` big.
    fn dragged(&self, at: Vec2, canvas: (f32, f32)) -> Option<[f32; 4]> {
        let (grab, start, from, snap) = self.drag?;
        let d = at - from;
        let [mut l, mut t, mut r, mut b] = start;
        match grab {
            Grab::Move => {
                l += d.x;
                r += d.x;
                t += d.y;
                b += d.y;
            }
            Grab::Left => l += d.x,
            Grab::Right => r += d.x,
            Grab::Top => t += d.y,
            Grab::Bottom => b += d.y,
            Grab::TopLeft => {
                l += d.x;
                t += d.y;
            }
            Grab::TopRight => {
                r += d.x;
                t += d.y;
            }
            Grab::BottomLeft => {
                l += d.x;
                b += d.y;
            }
            Grab::BottomRight => {
                r += d.x;
                b += d.y;
            }
        }
        // snapping: the frame's sides to the canvas's sides and middle
        if self.snapping {
            let snap_to = |v: f32, targets: [f32; 3]| targets.into_iter().find(|&s| (s - v).abs() < snap).unwrap_or(v);
            let xs = [0.0, canvas.0 / 2.0, canvas.0];
            let ys = [0.0, canvas.1 / 2.0, canvas.1];
            if grab == Grab::Move {
                let (dl, dr) = (snap_to(l, xs) - l, snap_to(r, xs) - r);
                let dx = if dl != 0.0 { dl } else { dr };
                l += dx;
                r += dx;
                let (dt, db) = (snap_to(t, ys) - t, snap_to(b, ys) - b);
                let dy = if dt != 0.0 { dt } else { db };
                t += dy;
                b += dy;
            } else {
                (l, r, t, b) = (snap_to(l, xs), snap_to(r, xs), snap_to(t, ys), snap_to(b, ys));
            }
        }
        // a locked aspect follows the width
        let (sw, sh) = (start[2] - start[0], start[3] - start[1]);
        if self.aspect_locked && grab != Grab::Move && sw > 0.0 {
            let h = (r - l) * sh / sw;
            if matches!(grab, Grab::Top | Grab::TopLeft | Grab::TopRight) {
                t = b - h;
            } else {
                b = t + h;
            }
        }
        (r - l >= 1.0 && b - t >= 1.0).then_some([l, t, r, b])
    }

    fn set_rect(&mut self, [l, t, r, b]: [f32; 4]) {
        let (w, h) = ((r - l).max(1.0), (b - t).max(1.0));
        self.rect = Some([l, t, l + w, t + h]);
        if !self.resample {
            self.size = (w.round() as u32, h.round() as u32);
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts Crop and Resize (again): the whole canvas kept, unturned, at its size, the canvas
    /// fitted to the window and shrunk so the frame's handles are clear. Aspect lock and snapping
    /// stay as they were.
    pub fn crop_begin(&self) {
        self.fit_canvas_scaled(CROP_SCALE);
        let mut st = self.shared.lock();
        let (w, h, dpi) = (st.doc.width(), st.doc.height(), st.doc.dpi);
        let e = &mut st.crop_edit;
        e.rect = Some([0.0, 0.0, w as f32, h as f32]);
        e.angle = 0.0;
        e.resample = false;
        e.size = (w, h);
        e.dpi = dpi;
        e.drag = None;
    }

    /// The crop as it stands; none outside Crop and Resize.
    pub fn crop_shown(&self) -> Option<CropShown> {
        let st = self.shared.lock();
        let e = &st.crop_edit;
        let [l, t, r, b] = e.rect?;
        let corners = st.view.map_or_else(Vec::new, |v| {
            e.corners([l, t, r, b])
                .into_iter()
                .flat_map(|p| {
                    let w = v.to_screen().apply(p);
                    [w.x, w.y]
                })
                .collect()
        });
        Some(CropShown {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
            angle: e.angle,
            resample: e.resample,
            size_width: e.size.0,
            size_height: e.size.1,
            aspect_locked: e.aspect_locked,
            snapping: e.snapping,
            dpi: e.dpi,
            corners,
        })
    }

    /// A press on the frame at window point (`x`, `y`): near a corner or a side's middle it takes
    /// that, anywhere else the whole frame.
    pub fn crop_press(&self, x: f32, y: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(rect) = st.crop_edit.rect else { return };
        let (Some(view), Some(from)) = (st.view, st.window_to_canvas(x, y)) else { return };
        let reach = REACH * st.display_scale;
        let at = Vec2::new(x, y);
        let cs = st.crop_edit.corners(rect).map(|p| view.to_screen().apply(p));
        let mid = |a: Vec2, b: Vec2| Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let near = |p: Vec2| (p - at).length() < reach;
        let [tl, tr, br, bl] = cs;
        let grab = if near(tl) {
            Grab::TopLeft
        } else if near(tr) {
            Grab::TopRight
        } else if near(br) {
            Grab::BottomRight
        } else if near(bl) {
            Grab::BottomLeft
        } else if near(mid(tl, bl)) {
            Grab::Left
        } else if near(mid(tl, tr)) {
            Grab::Top
        } else if near(mid(tr, br)) {
            Grab::Right
        } else if near(mid(br, bl)) {
            Grab::Bottom
        } else {
            Grab::Move
        };
        // canvas pixels a few window points cover, for snapping
        let snap = st.window_to_canvas(x + SNAP * st.display_scale, y).map_or(0.0, |p| (p - from).length());
        st.crop_edit.drag = Some((grab, rect, from, snap));
    }

    /// The drag on the frame reached window point (`x`, `y`).
    pub fn crop_drag(&self, x: f32, y: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(at) = st.window_to_canvas(x, y) else { return };
        let canvas = (st.doc.width() as f32, st.doc.height() as f32);
        if let Some(rect) = st.crop_edit.dragged(at, canvas) {
            st.crop_edit.set_rect(rect);
        }
    }

    pub fn crop_release(&self) {
        self.shared.lock().crop_edit.drag = None;
    }

    /// A width or height typed in: resampling, it is the new size; otherwise the kept
    /// rectangle's. A locked aspect keeps the other in step.
    pub fn crop_set_size(&self, width: Option<u32>, height: Option<u32>) {
        let mut st = self.shared.lock();
        let e = &mut st.crop_edit;
        let Some([l, t, _, _]) = e.rect else { return };
        let (w0, h0) = e.size;
        let most = canvas_edge_most();
        let mut w = width.unwrap_or(w0).clamp(1, most);
        let mut h = height.unwrap_or(h0).clamp(1, most);
        if e.aspect_locked && w0 > 0 && h0 > 0 {
            if width.is_some() {
                h = ((w as f32 * h0 as f32 / w0 as f32).round() as u32).clamp(1, most);
            } else if height.is_some() {
                w = ((h as f32 * w0 as f32 / h0 as f32).round() as u32).clamp(1, most);
            }
        }
        e.size = (w, h);
        if !e.resample {
            e.rect = Some([l, t, l + w as f32, t + h as f32]);
        }
    }

    pub fn crop_set_dpi(&self, dpi: f32) {
        if dpi.is_finite() && dpi >= 1.0 {
            self.shared.lock().crop_edit.dpi = dpi.min(9999.0);
        }
    }

    /// The crop's turn, radians.
    pub fn crop_set_angle(&self, radians: f32) {
        if radians.is_finite() {
            self.shared.lock().crop_edit.angle = radians;
        }
    }

    /// Resampling on: what is kept is scaled to the size; off, the canvas is the rectangle's size.
    pub fn crop_set_resample(&self, on: bool) {
        let mut st = self.shared.lock();
        let e = &mut st.crop_edit;
        e.resample = on;
        if let Some([l, t, r, b]) = e.rect {
            e.size = ((r - l).round() as u32, (b - t).round() as u32);
        }
    }

    pub fn crop_set_snapping(&self, on: bool) {
        self.shared.lock().crop_edit.snapping = on;
    }

    pub fn crop_set_aspect_locked(&self, on: bool) {
        self.shared.lock().crop_edit.aspect_locked = on;
    }

    /// Ends Crop and Resize: `keep` crops as set (and sets the DPI), else the canvas goes back as
    /// it was; either way it is fitted to the window again (cancelled, easing back). True if the
    /// canvas changed; false too if it couldn't be cropped ([`CanvasSession::take_failure`]).
    pub fn crop_end(&self, keep: bool) -> bool {
        let (rect, angle, resample, size, dpi) = {
            let mut st = self.shared.lock();
            let e = &mut st.crop_edit;
            let Some(rect) = e.rect.take() else { return false };
            e.drag = None;
            (rect, e.angle, e.resample, e.size, e.dpi)
        };
        if !keep {
            self.ease_to_fit();
            return false;
        }
        self.fit_view();
        let (canvas_dpi, cw, ch) = {
            let st = self.shared.lock();
            (st.doc.dpi, st.doc.width(), st.doc.height())
        };
        let [l, t, r, b] = rect;
        let (w, h) = (r - l, b - t);
        // nothing to crop: the whole canvas, unturned, at its size
        let same = l == 0.0
            && t == 0.0
            && w.round() as u32 == cw
            && h.round() as u32 == ch
            && angle == 0.0
            && size == (cw, ch);
        let cropped = !same && self.crop_canvas(l, t, w, h, angle, resample, size.0, size.1);
        // the DPI too, unless a crop was wanted and couldn't be made: then nothing changes
        if (same || cropped) && (dpi - canvas_dpi).abs() > 0.01 {
            self.set_dpi(dpi);
        }
        cropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(rect: [f32; 4], grab: Grab, from: (f32, f32)) -> CropEdit {
        CropEdit { rect: Some(rect), drag: Some((grab, rect, Vec2::new(from.0, from.1), 10.0)), ..CropEdit::default() }
    }

    #[test]
    fn a_drag_moves_the_part_it_took() {
        let canvas = (1000.0, 800.0);
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Move, (300.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(350.0, 270.0), canvas), Some([150.0, 120.0, 550.0, 420.0]));
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::TopLeft, (100.0, 100.0));
        assert_eq!(e.dragged(Vec2::new(80.0, 130.0), canvas), Some([80.0, 130.0, 500.0, 400.0]));
        // never inside out
        let e = edit([100.0, 100.0, 500.0, 400.0], Grab::Right, (500.0, 250.0));
        assert_eq!(e.dragged(Vec2::new(50.0, 250.0), canvas), None);
    }

    #[test]
    fn snapping_and_a_locked_aspect() {
        let canvas = (1000.0, 800.0);
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Right, (500.0, 250.0));
        e.snapping = true;
        // near the canvas's middle (500): there
        assert_eq!(e.dragged(Vec2::new(505.0, 250.0), canvas), Some([100.0, 100.0, 500.0, 400.0]));
        // moved, the nearest side snaps and the other follows
        let mut e = edit([100.0, 100.0, 500.0, 400.0], Grab::Move, (300.0, 250.0));
        e.snapping = true;
        assert_eq!(e.dragged(Vec2::new(205.0, 250.0), canvas), Some([0.0, 100.0, 400.0, 400.0]));
        // a locked aspect follows the width, from the bottom (or the top, taken by it)
        let mut e = edit([0.0, 0.0, 400.0, 300.0], Grab::Right, (400.0, 150.0));
        e.aspect_locked = true;
        assert_eq!(e.dragged(Vec2::new(800.0, 150.0), canvas), Some([0.0, 0.0, 800.0, 600.0]));
        let mut e = edit([0.0, 100.0, 400.0, 400.0], Grab::TopRight, (400.0, 100.0));
        e.aspect_locked = true;
        assert_eq!(e.dragged(Vec2::new(200.0, 100.0), canvas), Some([0.0, 250.0, 200.0, 400.0]));
    }
}
