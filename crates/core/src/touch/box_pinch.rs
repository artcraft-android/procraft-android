//! Two fingers in a transform. With one of them on the box, what it holds goes with them: moved,
//! turned and scaled as a whole, whichever kind of transform is on (the box stays the shape it
//! is); a pinch set off the box moves the canvas, as a warp's always does. Held still and lifted,
//! they are the tap they would be anywhere.

use super::{CanvasSession, Mode, TouchCommand, Touches, VIEW_SLOP, dist};

impl CanvasSession {
    /// A second finger came down: on the box, what it holds is theirs once they move. A first
    /// finger already moving it keeps what it did; any other drag goes back.
    pub(super) fn second_finger_on_box(&self, t: &mut Touches, out: &mut Vec<TouchCommand>) {
        if t.mode == Mode::Gesture {
            // a third finger: what two made stays
            self.end_box_pinch(t, false, out);
        }
        if t.mode != Mode::Transform {
            return;
        }
        t.box_pinch = self.fingers_on_box(t.down.iter().map(|d| d.at)).then_some(None);
        if std::mem::take(&mut t.transform_grabbed) {
            let kept = t.box_pinch.is_some() && t.transform_handle == crate::TransformHandle::Move;
            let transform = if kept { self.transform_release() } else { self.transform_cancel_drag() };
            out.push(TouchCommand::TransformMoved { transform });
        }
    }

    /// One step of two fingers on the box: past a little way they take what it holds, from where
    /// they are then so it doesn't jump, and it follows them.
    pub(super) fn pinch_box(&self, t: &mut Touches, out: &mut Vec<TouchCommand>) {
        let Some((a, b)) = t.gesture_pair else { return };
        let at = |id: u64| t.down.iter().find(|d| d.id == id).map(|d| d.at);
        let (Some(a), Some(b)) = (at(a), at(b)) else { return };
        match t.box_pinch {
            Some(Some(from)) => {
                let transform = self.transform_pinch(from, [a, b]);
                out.push(TouchCommand::TransformMoved { transform });
            }
            Some(None) if t.down.iter().map(|d| dist(d.at, d.from)).sum::<f32>() >= VIEW_SLOP * t.scale => {
                self.transform_pinch_grab(a);
                t.box_pinch = Some(Some([a, b]));
            }
            _ => {}
        }
    }

    /// The fingers on the box part (or were taken back: it goes back too). Returns whether they
    /// had taken what it holds.
    pub(super) fn end_box_pinch(&self, t: &mut Touches, cancelled: bool, out: &mut Vec<TouchCommand>) -> bool {
        let Some(pinch) = t.box_pinch.take() else { return false };
        if pinch.is_some() {
            let transform = if cancelled { self.transform_cancel_drag() } else { self.transform_release() };
            out.push(TouchCommand::TransformMoved { transform });
        }
        pinch.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::touch::TransformTouch;
    use crate::{CanvasSession, PenSample, PointerKind, TouchCommand};
    use procraft_engine::geom::Affine;
    use procraft_engine::transform::{Transform, TransformMode};
    use procraft_engine::view::View;

    fn at(x: f32, y: f32, ms: f64) -> Vec<PenSample> {
        vec![PenSample { x, y, pressure: 0.5, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }]
    }

    /// A 400 x 300 box at (100, 100), seen as it is.
    fn open(mode: TransformMode) -> std::sync::Arc<CanvasSession> {
        let s = CanvasSession::new(600, 600);
        {
            let mut st = s.shared.lock();
            st.view = Some(View::from_affine(Affine::IDENTITY));
            st.transform = Some(Transform::new(100.0, 100.0, 400.0, 300.0, Vec::new()));
            st.transform_mode = mode;
        }
        s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false }));
        s
    }

    fn corners(s: &CanvasSession) -> Vec<f32> {
        s.shared.lock().transform_box().map(|b| b.corners).unwrap_or_default()
    }

    fn view(s: &CanvasSession) -> Vec<f32> {
        s.view_matrix()
    }

    /// Two fingers from 20 either side of (`x`, `y`) to `spread` times as far apart, in steps.
    fn pinch(s: &CanvasSession, x: f32, y: f32, spread: f32) -> Vec<TouchCommand> {
        s.touch_down(1, PointerKind::Finger, at(x - 20.0, y, 0.0));
        let mut out = s.touch_down(2, PointerKind::Finger, at(x + 20.0, y, 4.0));
        for i in 1..=10 {
            let half = 20.0 + 20.0 * (spread - 1.0) * i as f32 / 10.0;
            let ms = 4.0 + 16.0 * f64::from(i);
            out.extend(s.touch_move(1, at(x - half, y, ms)));
            out.extend(s.touch_move(2, at(x + half, y, ms)));
        }
        out.extend(s.touch_up(1, at(x - 20.0 * spread, y, 200.0)));
        out.extend(s.touch_up(2, at(x + 20.0 * spread, y, 204.0)));
        out
    }

    #[test]
    fn two_fingers_on_the_box_scale_what_it_holds_and_leave_the_canvas() {
        for mode in [TransformMode::Uniform, TransformMode::Freeform, TransformMode::Distort] {
            let s = open(mode);
            let before = (corners(&s), view(&s));
            let out = pinch(&s, 200.0, 150.0, 3.0);
            assert!(out.iter().any(|c| matches!(c, TouchCommand::TransformMoved { .. })), "{out:?}");
            assert!(!out.contains(&TouchCommand::ViewMoved), "{out:?}");
            assert_eq!(view(&s), before.1);
            let now = corners(&s);
            assert!(now[2] - now[0] > 400.0 * 1.5, "{now:?}");
            // about the fingers, still upright, as one step of the transform's undo
            assert!(((200.0 - now[0]) / (now[2] - now[0]) - 0.25).abs() < 0.02, "{now:?}");
            assert!((now[1] - now[3]).abs() < 1e-3, "{now:?}");
            assert!(s.shared.lock().transform_box().is_some_and(|b| b.can_undo));
        }
    }

    #[test]
    fn one_finger_on_the_box_is_enough() {
        let s = open(TransformMode::Uniform);
        let before = view(&s);
        pinch(&s, 90.0, 150.0, 2.0);
        assert_eq!(view(&s), before);
    }

    #[test]
    fn two_fingers_off_the_box_move_the_canvas_and_leave_it() {
        let s = open(TransformMode::Uniform);
        let (box_was, view_was) = (corners(&s), view(&s));
        pinch(&s, 200.0, 500.0, 3.0);
        assert_eq!(corners(&s), box_was);
        assert_ne!(view(&s), view_was);
    }

    #[test]
    fn a_warp_s_two_fingers_always_move_the_canvas() {
        let s = open(TransformMode::Warp);
        let (box_was, view_was) = (corners(&s), view(&s));
        pinch(&s, 200.0, 150.0, 3.0);
        assert_eq!(corners(&s), box_was);
        assert_ne!(view(&s), view_was);
    }

    #[test]
    fn two_fingers_turning_turn_it_with_them() {
        let s = open(TransformMode::Uniform);
        s.touch_down(1, PointerKind::Finger, at(170.0, 150.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(230.0, 150.0, 4.0));
        // a little way first, then a quarter turn about the middle, the spread kept
        s.touch_move(1, at(160.0, 150.0, 20.0));
        s.touch_move(2, at(240.0, 150.0, 20.0));
        s.touch_move(1, at(200.0, 110.0, 40.0));
        s.touch_move(2, at(200.0, 190.0, 40.0));
        s.touch_up(1, at(200.0, 110.0, 60.0));
        s.touch_up(2, at(200.0, 190.0, 64.0));
        let c = corners(&s);
        // the top left corner (-100, -50 from the middle) turned to (50, -100)
        assert!((c[0] - 250.0).abs() < 1.0 && (c[1] - 50.0).abs() < 1.0, "{c:?}");
    }

    #[test]
    fn two_fingers_tapped_on_the_box_are_still_a_tap() {
        let s = open(TransformMode::Uniform);
        let before = corners(&s);
        s.touch_down(1, PointerKind::Finger, at(180.0, 150.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(220.0, 150.0, 4.0));
        let mut out = s.touch_up(1, at(180.0, 150.0, 60.0));
        out.extend(s.touch_up(2, at(220.0, 150.0, 64.0)));
        assert!(out.contains(&TouchCommand::Undo), "{out:?}");
        assert_eq!(corners(&s), before);
    }

    #[test]
    fn a_move_a_second_finger_joins_is_kept_and_a_node_s_drag_goes_back() {
        let s = open(TransformMode::Uniform);
        s.touch_down(1, PointerKind::Finger, at(300.0, 250.0, 0.0));
        s.touch_move(1, at(340.0, 250.0, 16.0));
        s.touch_move(1, at(360.0, 250.0, 32.0));
        s.touch_down(2, PointerKind::Finger, at(400.0, 250.0, 40.0));
        let moved = corners(&s);
        assert_eq!(moved[0], 120.0, "{moved:?}");
        s.touch_up(1, at(360.0, 250.0, 60.0));
        s.touch_up(2, at(400.0, 250.0, 64.0));

        let s = open(TransformMode::Uniform);
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_move(1, at(60.0, 80.0, 16.0));
        s.touch_move(1, at(40.0, 60.0, 32.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 250.0, 40.0));
        assert_eq!(corners(&s)[..2], [100.0, 100.0]);
    }

    #[test]
    fn a_pinch_taken_back_puts_it_back() {
        let s = open(TransformMode::Uniform);
        let before = corners(&s);
        s.touch_down(1, PointerKind::Finger, at(180.0, 150.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(220.0, 150.0, 4.0));
        s.touch_move(1, at(150.0, 150.0, 20.0));
        s.touch_move(1, at(120.0, 150.0, 36.0));
        assert_ne!(corners(&s), before);
        s.touch_cancel();
        assert_eq!(corners(&s), before);
    }
}
