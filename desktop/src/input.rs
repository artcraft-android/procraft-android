//! The pointer on the canvas. The mouse, a pen (with its pressure, where the platform gives it)
//! and a finger draw strokes; the middle button, the left with space held, the wheel, pinches
//! and two fingers move the view.

use std::time::Instant;

use procraft_core::{CanvasSession, PenSample, PointerKind};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::keyboard::{Key, NamedKey};

/// How much a notch of the wheel zooms by.
const WHEEL_ZOOM: f32 = 1.15;
/// How much a pixel of scrolling zooms by, with Ctrl (or ⌘) held.
const SCROLL_ZOOM: f32 = 0.005;

pub struct Input {
    clock: Instant,
    /// Where the mouse is (window pixels).
    cursor: Option<(f32, f32)>,
    /// The mouse is drawing a stroke.
    drawing: bool,
    /// The view follows the mouse (the middle button, or the left with space held).
    panning: bool,
    /// Space is held: the left button moves the view.
    space: bool,
    /// Ctrl (⌘ on the Mac) is held: scrolling zooms.
    command: bool,
    /// The touch drawing a stroke: its id, and whether it is a pen's.
    stroke_touch: Option<(u64, bool)>,
    /// Fingers down, and where (window pixels).
    fingers: Vec<(u64, (f32, f32))>,
}

impl Default for Input {
    fn default() -> Input {
        Input {
            clock: Instant::now(),
            cursor: None,
            drawing: false,
            panning: false,
            space: false,
            command: false,
            stroke_touch: None,
            fingers: Vec::new(),
        }
    }
}

impl Input {
    /// Space is held, so a press moves the view.
    pub fn moving_view(&self) -> bool {
        self.space || self.panning
    }

    /// Acts on `event` (`consumed` when the interface took it; `over` tells whether a point is
    /// on the interface rather than the canvas). True if anything changed.
    pub fn on_event(
        &mut self,
        session: &CanvasSession,
        event: &WindowEvent,
        consumed: bool,
        over: impl Fn((f32, f32)) -> bool,
    ) -> bool {
        match event {
            WindowEvent::ModifiersChanged(m) => {
                let state = m.state();
                self.command = if cfg!(target_os = "macos") { state.super_key() } else { state.control_key() };
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::Space) => {
                self.space = event.state == ElementState::Pressed && !consumed;
                if !self.space && self.panning && !self.drawing {
                    self.panning = false;
                }
                true
            }
            WindowEvent::Focused(false) => {
                // keys let go elsewhere never come back up here
                self.space = false;
                self.command = false;
                false
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                let last = self.cursor.replace(p);
                if self.drawing {
                    session.stroke_add(vec![self.sample(p, 1.0)]);
                    true
                } else if let (true, Some(l)) = (self.panning, last) {
                    session.view_gesture(p.0, p.1, p.0 - l.0, p.1 - l.1, 1.0, 0.0);
                    true
                } else {
                    false
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.on_button(session, *state, *button, consumed, &over),
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(p) = self.cursor.filter(|p| !consumed && !over(*p)) else { return false };
                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        session.view_gesture(p.0, p.1, 0.0, 0.0, WHEEL_ZOOM.powf(*y), 0.0);
                    }
                    MouseScrollDelta::PixelDelta(d) if self.command => {
                        session.view_gesture(p.0, p.1, 0.0, 0.0, (d.y as f32 * SCROLL_ZOOM).exp(), 0.0);
                    }
                    MouseScrollDelta::PixelDelta(d) => {
                        session.view_gesture(p.0, p.1, d.x as f32, d.y as f32, 1.0, 0.0);
                    }
                }
                true
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let Some(p) = self.cursor.filter(|p| !over(*p)) else { return false };
                session.view_gesture(p.0, p.1, 0.0, 0.0, (1.0 + *delta as f32).max(0.01), 0.0);
                true
            }
            WindowEvent::RotationGesture { delta, .. } => {
                let Some(p) = self.cursor.filter(|p| !over(*p)) else { return false };
                // turned anticlockwise is positive; the view turns clockwise on the screen
                session.view_gesture(p.0, p.1, 0.0, 0.0, 1.0, -delta.to_radians());
                true
            }
            WindowEvent::Touch(touch) => self.on_touch(session, touch, consumed, &over),
            _ => false,
        }
    }

    fn on_button(
        &mut self,
        session: &CanvasSession,
        state: ElementState,
        button: MouseButton,
        consumed: bool,
        over: &impl Fn((f32, f32)) -> bool,
    ) -> bool {
        let Some(p) = self.cursor else { return false };
        match (button, state) {
            (MouseButton::Left | MouseButton::Middle, ElementState::Pressed)
                if !self.drawing && !consumed && !over(p) && (self.space || button == MouseButton::Middle) =>
            {
                self.panning = true;
                false
            }
            (MouseButton::Left, ElementState::Pressed)
                if !self.drawing && !self.panning && self.stroke_touch.is_none() && !consumed && !over(p) =>
            {
                self.drawing = true;
                session.stroke_begin(PointerKind::Mouse, vec![self.sample(p, 1.0)]);
                true
            }
            (MouseButton::Left, ElementState::Released) if self.drawing => {
                self.drawing = false;
                session.stroke_end(vec![self.sample(p, 1.0)]);
                true
            }
            (MouseButton::Left | MouseButton::Middle, ElementState::Released) => {
                self.panning = false;
                false
            }
            _ => false,
        }
    }

    fn on_touch(
        &mut self,
        session: &CanvasSession,
        touch: &Touch,
        consumed: bool,
        over: &impl Fn((f32, f32)) -> bool,
    ) -> bool {
        let p = (touch.location.x as f32, touch.location.y as f32);
        // the platform gives a pen's pressure, and none for a finger
        let pen = touch.force.map(|f| f.normalized() as f32);
        let id = touch.id;
        match touch.phase {
            TouchPhase::Started => {
                if let Some(pressure) = pen {
                    if self.stroke_touch.is_some() || self.drawing || consumed || over(p) {
                        return false;
                    }
                    self.stroke_touch = Some((id, true));
                    session.stroke_begin(PointerKind::Stylus, vec![self.sample(p, pressure)]);
                    return true;
                }
                self.fingers.retain(|(f, _)| *f != id);
                self.fingers.push((id, p));
                match self.fingers.len() {
                    1 if self.stroke_touch.is_none() && !self.drawing && !consumed && !over(p) => {
                        self.stroke_touch = Some((id, false));
                        session.stroke_begin(PointerKind::Finger, vec![self.sample(p, 1.0)]);
                        true
                    }
                    // a second finger moves the view: the first one's stroke is taken back
                    2 => {
                        if let Some((_, false)) = self.stroke_touch.take() {
                            session.stroke_cancel();
                        }
                        true
                    }
                    _ => false,
                }
            }
            TouchPhase::Moved => {
                if self.stroke_touch.is_some_and(|(t, _)| t == id) {
                    session.stroke_add(vec![self.sample(p, pen.unwrap_or(1.0))]);
                    return true;
                }
                let before: Vec<(f32, f32)> = self.fingers.iter().map(|(_, at)| *at).collect();
                let Some(at) = self.fingers.iter_mut().find(|(f, _)| *f == id) else { return false };
                at.1 = p;
                let after: Vec<(f32, f32)> = self.fingers.iter().map(|(_, at)| *at).collect();
                if let ([a0, b0], [a1, b1]) = (before.as_slice(), after.as_slice()) {
                    two_finger_step(session, (*a0, *b0), (*a1, *b1));
                    return true;
                }
                false
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.fingers.retain(|(f, _)| *f != id);
                if self.stroke_touch.is_some_and(|(t, _)| t == id) {
                    self.stroke_touch = None;
                    if touch.phase == TouchPhase::Ended {
                        session.stroke_end(vec![self.sample(p, pen.unwrap_or(1.0))]);
                    } else {
                        session.stroke_cancel();
                    }
                    return true;
                }
                false
            }
        }
    }

    fn sample(&self, (x, y): (f32, f32), pressure: f32) -> PenSample {
        PenSample {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            tilt: 0.0,
            orientation: 0.0,
            time_ms: self.clock.elapsed().as_secs_f64() * 1000.0,
        }
    }
}

/// The view follows two fingers from `before` to `after`: moved with their middle, scaled with
/// their spread and turned with the line between them.
fn two_finger_step(session: &CanvasSession, before: ((f32, f32), (f32, f32)), after: ((f32, f32), (f32, f32))) {
    let mid = |(a, b): ((f32, f32), (f32, f32))| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let spread = |(a, b): ((f32, f32), (f32, f32))| (b.0 - a.0).hypot(b.1 - a.1);
    let angle = |(a, b): ((f32, f32), (f32, f32))| (b.1 - a.1).atan2(b.0 - a.0);
    let (m0, m1) = (mid(before), mid(after));
    let scale = if spread(before) > 1.0 { spread(after) / spread(before) } else { 1.0 };
    session.view_gesture(m0.0, m0.1, m1.0 - m0.0, m1.1 - m0.1, scale, angle(after) - angle(before));
}
