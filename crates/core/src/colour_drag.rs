//! A colour dragged out to be dropped (ColorDrop), as the apps' colour button and swatches see
//! its pointer: it follows the pointer until the pointer rests a moment where a drop is taken; then
//! it stays there, fills at once, and sliding sets the threshold, the fill redone as it goes. Let
//! go, it fills where it is (the apps pour; this only says when and with what).

use std::sync::{Arc, Mutex, MutexGuard};

use crate::touch::{POUR_SLIDE_GENTLER, drag_step};

/// Resting this long (ms) makes the dragged colour stay where it is.
const REST_MS: f64 = 300.0;
/// A pen moving less than this (points, all told) still rests; a finger must keep still (a little
/// under a point, for a touchscreen's jitter).
const PEN_REST: f32 = 3.0;
const FINGER_REST: f32 = 0.5;

/// Where a dragged colour is after a move, and what the move did.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct DragStep {
    /// Where the colour's dot is (points): the pointer, or where it came to rest.
    pub dot_x: f32,
    pub dot_y: f32,
    /// It rests where it is: sliding sets the threshold.
    pub resting: bool,
    pub threshold: f32,
    /// The move changed the threshold: the fill is redone with it (begun, the first time).
    pub slid: bool,
}

struct Drag {
    pen: bool,
    dot: (f32, f32),
    last: (f32, f32, f64),
    /// Since when the pointer has rested, and how far it has moved since (points).
    rest_since: f64,
    moved: f32,
    /// It came to rest where no drop is taken: it waits for a move before resting again.
    refused: bool,
    resting: bool,
    threshold: f32,
}

/// A colour being dragged out: made as the drag starts, told each move, asked when it comes to
/// rest.
#[derive(uniffi::Object)]
pub struct ColourDrag {
    drag: Mutex<Drag>,
}

#[uniffi::export]
impl ColourDrag {
    /// Dragging starts at (`x`, `y`) (points) at `ms`, by a pen or not, with the kept `threshold`.
    #[uniffi::constructor]
    pub fn new(x: f32, y: f32, ms: f64, pen: bool, threshold: f32) -> Arc<ColourDrag> {
        Arc::new(ColourDrag {
            drag: Mutex::new(Drag {
                pen,
                dot: (x, y),
                last: (x, y, ms),
                rest_since: ms,
                moved: 0.0,
                refused: false,
                resting: false,
                threshold: threshold.clamp(0.0, 1.0),
            }),
        })
    }

    /// The pointer moved to (`x`, `y`) at `ms`: the dot goes with it, or, resting, the threshold
    /// slides with it (faster the faster it goes, as Continue filling's touch slides it).
    pub fn moved(&self, x: f32, y: f32, ms: f64) -> DragStep {
        let mut d = self.lock();
        let (last_x, last_y, last_ms) = d.last;
        d.last = (x, y, ms);
        let mut slid = false;
        if d.resting {
            let dx = x - last_x;
            if dx != 0.0 {
                let t = (d.threshold + drag_step(dx, ms - last_ms) / POUR_SLIDE_GENTLER).clamp(0.0, 1.0);
                slid = t != d.threshold;
                d.threshold = t;
            }
        } else {
            d.dot = (x, y);
            d.moved += (x - last_x).hypot(y - last_y);
            if d.moved > if d.pen { PEN_REST } else { FINGER_REST } {
                d.rest_since = ms;
                d.moved = 0.0;
                d.refused = false;
            }
        }
        d.step(slid)
    }

    /// When (ms) the pointer, if it stays, comes to rest; none once it has, or while it waits for
    /// a move after resting where no drop is taken.
    pub fn rest_due(&self) -> Option<f64> {
        let d = self.lock();
        (!d.resting && !d.refused).then_some(d.rest_since + REST_MS)
    }

    /// The time is `ms`, the dot where a drop is taken or not (`droppable`): whether it now
    /// rests (from here on sliding sets the threshold). Where no drop is taken it isn't asked
    /// again until the pointer has moved.
    pub fn rest(&self, ms: f64, droppable: bool) -> bool {
        let mut d = self.lock();
        if d.resting || d.refused || ms < d.rest_since + REST_MS {
            return false;
        }
        if !droppable {
            d.refused = true;
            return false;
        }
        d.resting = true;
        true
    }

    /// Where the dot is and the threshold, as things stand.
    pub fn now(&self) -> DragStep {
        self.lock().step(false)
    }
}

impl ColourDrag {
    fn lock(&self) -> MutexGuard<'_, Drag> {
        self.drag.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drag {
    fn step(&self, slid: bool) -> DragStep {
        DragStep { dot_x: self.dot.0, dot_y: self.dot.1, resting: self.resting, threshold: self.threshold, slid }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_colour_rests_after_a_moment_still_and_then_slides_the_threshold() {
        let drag = ColourDrag::new(100.0, 10.0, 0.0, true, 0.5);
        // moving, it follows and doesn't rest
        let s = drag.moved(200.0, 300.0, 100.0);
        assert_eq!((s.dot_x, s.dot_y, s.resting, s.slid), (200.0, 300.0, false, false));
        assert_eq!(drag.rest_due(), Some(400.0));
        assert!(!drag.rest(399.0, true));
        // a pen's small wobble still counts as resting; somewhere a drop isn't taken, it doesn't,
        // and it isn't due again until the pen moves
        drag.moved(201.0, 301.0, 200.0);
        assert!(!drag.rest(400.0, false));
        assert_eq!(drag.rest_due(), None);
        assert!(!drag.rest(450.0, true));
        drag.moved(202.0, 301.0, 460.0);
        assert_eq!(drag.rest_due(), None, "a wobble isn't a move");
        drag.moved(206.0, 301.0, 470.0);
        assert_eq!(drag.rest_due(), Some(770.0));
        assert!(drag.rest(770.0, true));
        assert_eq!(drag.rest_due(), None);
        // resting, the dot stays and a slide right raises the threshold, left lowers it
        let s = drag.moved(260.0, 320.0, 800.0);
        assert_eq!((s.dot_x, s.dot_y, s.resting), (206.0, 301.0, true));
        assert!(s.slid && s.threshold > 0.5);
        let up = s.threshold;
        let s = drag.moved(150.0, 320.0, 1100.0);
        assert!(s.slid && s.threshold < up);
        // straight down changes nothing
        assert!(!drag.moved(150.0, 400.0, 1200.0).slid);
    }

    #[test]
    fn a_finger_must_keep_still_to_rest() {
        let drag = ColourDrag::new(0.0, 0.0, 0.0, false, 0.5);
        drag.moved(2.0, 0.0, 250.0);
        assert!(!drag.rest(300.0, true), "the move started the wait again");
        assert!(drag.rest(550.0, true));
    }
}
