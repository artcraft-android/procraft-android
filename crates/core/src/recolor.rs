//! Recolor (the QuickMenu's): the area under a reticle takes a colour. Dragged about, the reticle
//! moves and the fill follows it; a tap keeps the fill and starts again where it lands; its Flood
//! says how far from the colour under the reticle the fill reaches. It ends keeping what it
//! filled, each fill kept one "Recolor" undo step. The platforms draw the reticle (at
//! [`CanvasSession::recolor_point`]) and the Flood toolbar; the touches say where it is dragged or
//! tapped (touch.rs's Recolor mode) and the app moves it there ([`CanvasSession::recolor_move_to`],
//! [`CanvasSession::recolor_tap`]), at once or on a thread of its own. Its fills are the pour's
//! (pour.rs). Undo takes back the open fill first, and a
//! redo makes it again over what it brings back (lib.rs's `undo`, `redo`).

use procraft_engine::geom::Vec2;

use crate::CanvasSession;

/// The undo step a kept fill makes.
pub(crate) const STEP: &str = "Recolor";

/// Recolor while it is on: where the reticle is (canvas pixels; none off the canvas), whether a
/// fill is open under it, and the colour and Flood it fills with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Recolor {
    point: Option<Vec2>,
    open: bool,
    rgb: [f32; 3],
    threshold: f32,
}

impl CanvasSession {
    fn recolor_state(&self) -> Option<Recolor> {
        self.shared.lock().recolor
    }

    fn keep_recolor(&self, r: Recolor) {
        self.shared.lock().recolor = Some(r);
    }

    /// A fill under window pixel `at` with `r`'s colour and Flood; false where there is nothing to
    /// fill.
    fn recolor_fill(&self, (x, y): (f32, f32), r: &Recolor) -> bool {
        let [red, green, blue] = r.rgb;
        self.pour_begin(x, y, red, green, blue, r.threshold)
    }

    fn to_canvas(&self, (x, y): (f32, f32)) -> Option<Vec2> {
        self.shared.lock().window_to_canvas(x, y)
    }

    fn to_window(&self, p: Vec2) -> Option<(f32, f32)> {
        let st = self.shared.lock();
        let at = st.view?.to_screen().apply(p);
        Some((at.x, at.y))
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Recolor on, the reticle at window pixel (`x`, `y`) (the middle of the window, say),
    /// filling with (`r`, `g`, `b`) as far as `threshold` (0..1). Already on: nothing changes.
    pub fn recolor_start(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) {
        if self.recolor_state().is_some() {
            return;
        }
        let mut recolor = Recolor {
            point: self.to_canvas((x, y)),
            open: false,
            rgb: [r, g, b],
            threshold: threshold.clamp(0.0, 1.0),
        };
        recolor.open = recolor.point.is_some() && self.recolor_fill((x, y), &recolor);
        self.keep_recolor(recolor);
    }

    /// Whether Recolor is on.
    pub fn recolor_on(&self) -> bool {
        self.recolor_state().is_some()
    }

    /// Where the reticle is in the window (x, y, pixels), while Recolor is on and it is on the
    /// canvas.
    pub fn recolor_point(&self) -> Option<Vec<f32>> {
        let (x, y) = self.to_window(self.recolor_state()?.point?)?;
        Some(vec![x, y])
    }

    /// The reticle dragged by (`dx`, `dy`) window pixels: the fill follows it.
    pub fn recolor_drag(&self, dx: f32, dy: f32) {
        let Some(from) = self.recolor_state().and_then(|r| r.point).and_then(|p| self.to_window(p)) else { return };
        self.recolor_move_to(from.0 + dx, from.1 + dy);
    }

    /// The reticle moved to window pixel (`x`, `y`): the fill follows it (only the last of several
    /// moves need be made).
    pub fn recolor_move_to(&self, x: f32, y: f32) {
        let Some(mut recolor) = self.recolor_state() else { return };
        let Some(moved) = self.to_canvas((x, y)) else { return };
        recolor.point = Some(moved);
        if recolor.open {
            self.pour_move(x, y);
        } else {
            recolor.open = self.recolor_fill((x, y), &recolor);
        }
        self.keep_recolor(recolor);
    }

    /// A tap at window pixel (`x`, `y`): the fill so far is kept, and the reticle starts again
    /// there.
    pub fn recolor_tap(&self, x: f32, y: f32) {
        let Some(mut recolor) = self.recolor_state() else { return };
        if recolor.open {
            self.pour_end(true, Some(STEP.to_owned()));
        }
        recolor.point = self.to_canvas((x, y));
        recolor.open = recolor.point.is_some() && self.recolor_fill((x, y), &recolor);
        self.keep_recolor(recolor);
    }

    /// The Flood moved to `threshold` (0..1): the fill redone with it.
    pub fn recolor_threshold(&self, threshold: f32) {
        let Some(mut recolor) = self.recolor_state() else { return };
        recolor.threshold = threshold.clamp(0.0, 1.0);
        if recolor.open {
            self.pour_threshold(recolor.threshold);
        }
        self.keep_recolor(recolor);
    }

    /// The colour changed to (`r`, `g`, `b`): the area under the reticle takes it.
    pub fn recolor_colour(&self, r: f32, g: f32, b: f32) {
        let Some(mut recolor) = self.recolor_state() else { return };
        recolor.rgb = [r, g, b];
        if recolor.open {
            self.pour_colour(r, g, b);
        }
        self.keep_recolor(recolor);
    }

    /// Undo or redo coming: the open fill steps aside (true if there was one, which an undo
    /// takes back on its own).
    pub fn recolor_step_aside(&self) -> bool {
        let Some(mut recolor) = self.recolor_state().filter(|r| r.open) else { return false };
        self.pour_end(false, None);
        recolor.open = false;
        self.keep_recolor(recolor);
        true
    }

    /// After a redo: the fill made again over what it brought back.
    pub fn recolor_again(&self) {
        let Some(mut recolor) = self.recolor_state() else { return };
        let Some(at) = recolor.point.and_then(|p| self.to_window(p)) else { return };
        recolor.open = self.recolor_fill(at, &recolor);
        self.keep_recolor(recolor);
    }

    /// Recolor ends, keeping what it filled.
    pub fn recolor_end(&self) {
        let Some(recolor) = self.recolor_state() else { return };
        if recolor.open {
            self.pour_end(true, Some(STEP.to_owned()));
        }
        self.shared.lock().recolor = None;
    }
}

#[cfg(test)]
mod tests {
    use procraft_engine::geom::Affine;
    use procraft_engine::view::View;

    use crate::CanvasSession;

    /// A session shown one to one at the window's corner.
    fn session() -> std::sync::Arc<CanvasSession> {
        let s = CanvasSession::new(64, 64);
        s.shared.lock().view = Some(View::from_affine(Affine::IDENTITY));
        s
    }

    #[test]
    fn the_reticle_starts_where_it_is_put_follows_a_drag_and_a_tap_moves_it() {
        let s = session();
        assert!(!s.recolor_on());
        s.recolor_start(32.0, 32.0, 1.0, 0.0, 0.0, 0.0);
        assert!(s.recolor_on());
        assert_eq!(s.recolor_point(), Some(vec![32.0, 32.0]));
        s.recolor_drag(10.0, -2.0);
        assert_eq!(s.recolor_point(), Some(vec![42.0, 30.0]));
        s.recolor_tap(5.0, 6.0);
        assert_eq!(s.recolor_point(), Some(vec![5.0, 6.0]));
        // nothing open to step aside here (no canvas to fill), and it ends
        assert!(!s.recolor_step_aside());
        s.recolor_end();
        assert!(!s.recolor_on());
        assert_eq!(s.recolor_point(), None);
    }
}
