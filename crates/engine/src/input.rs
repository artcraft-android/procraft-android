//! Input as the platform reports it.

use crate::geom::Vec2;

/// What touched the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Finger,
    Stylus,
    /// The eraser end of a pen.
    StylusEraser,
    Mouse,
}

impl Pointer {
    /// Whether the input reports pressure (fingers and mice don't).
    pub fn has_pressure(self) -> bool {
        matches!(self, Pointer::Stylus | Pointer::StylusEraser)
    }
}

/// One input sample, including the in-between samples the platform batched since the last event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Position in screen pixels (or canvas pixels once the view has mapped it).
    pub pos: Vec2,
    /// 0..1; ignored for inputs without pressure.
    pub pressure: f32,
    /// Angle from the screen's normal, radians (0 = upright, π/2 = flat).
    pub tilt: f32,
    /// Direction the pen leans towards, radians clockwise from screen-up.
    pub azimuth: f32,
    /// Milliseconds on a monotonic clock.
    pub time_ms: f64,
}

impl Sample {
    pub fn at(pos: Vec2, pressure: f32, time_ms: f64) -> Sample {
        Sample { pos, pressure, tilt: 0.0, azimuth: 0.0, time_ms }
    }

    pub fn is_finite(&self) -> bool {
        self.pos.is_finite() && self.pressure.is_finite() && self.time_ms.is_finite()
    }
}
