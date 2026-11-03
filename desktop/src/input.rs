//! The window's pointers handed to the session's touches (crates/core's `touch`, shared with the
//! Android app), which paint, move the canvas and say what the app should do. The mouse adds what
//! it can't do by touch: the wheel zooms, and the middle button (or space) moves the canvas; on a
//! model it turns it, as a finger does, and with Shift moves it.

use std::time::{Duration, Instant};

use procraft_core::{CanvasSession, PenSample, PointerKind, TouchCommand};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::keyboard::{Key, NamedKey};

/// The eyedropper's window, points.
pub const PICK_WINDOW: f32 = 54.0;
/// How much a notch of the wheel zooms by.
const WHEEL_ZOOM: f32 = 1.15;
/// How much a pixel of scrolling zooms by, with Ctrl (or ⌘) held.
const SCROLL_ZOOM: f32 = 0.005;
/// How far a notch of the wheel turns the canvas with Shift held (radians, 5°).
const WHEEL_TURN: f32 = 5.0 * std::f32::consts::PI / 180.0;
/// How far a pixel of scrolling turns it with Shift held (radians, a quarter of a degree).
const SCROLL_TURN: f32 = 0.25 * std::f32::consts::PI / 180.0;

/// The mouse's pointer id: touches have the platform's.
const MOUSE: u64 = u64::MAX;

pub struct Input {
    clock: Instant,
    /// What the touches asked, for the app to do at the next frame.
    pub commands: Vec<TouchCommand>,
    /// The pointers down on the canvas.
    down: Vec<u64>,
    /// The mouse: where it is, whether it is over the window, and whether it (or space) moves the
    /// view.
    cursor: Option<(f32, f32)>,
    inside: bool,
    panning: bool,
    /// The pen moving the view (pressed with space held), as the mouse does.
    pen_panning: Option<u64>,
    /// The brush cursor follows the mouse hovering, so its moves draw a frame.
    pub hover_shown: bool,
    space: bool,
    command_key: bool,
    shift: bool,
    /// A model is shown (not its 2D texture): dragging the view turns it, as one finger does (the
    /// touches').
    pub turns_model: bool,
}

impl Default for Input {
    fn default() -> Input {
        Input {
            clock: Instant::now(),
            commands: Vec::new(),
            down: Vec::new(),
            cursor: None,
            inside: false,
            panning: false,
            pen_panning: None,
            hover_shown: false,
            space: false,
            command_key: false,
            shift: false,
            turns_model: false,
        }
    }
}

impl Input {
    /// Where the mouse hovers (window pixels): over the window, nothing pressed, the view not
    /// moving.
    /// Where the mouse (or a pen moving the pointer) last was over the window, window pixels.
    pub fn cursor(&self) -> Option<(f32, f32)> {
        self.cursor
    }

    pub fn hovering(&self) -> Option<(f32, f32)> {
        self.cursor.filter(|_| self.inside && self.down.is_empty() && !self.panning && !self.space)
    }

    /// Nothing is pressed on the canvas and the view isn't moving.
    pub fn idle(&self) -> bool {
        self.down.is_empty() && !self.panning
    }

    /// Space is held or the mouse moves the view.
    pub fn moving_view(&self) -> bool {
        self.space || self.panning
    }

    fn now_ms(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    /// When the touches next need [`Input::tick`], if ever.
    pub fn next_deadline(&self, session: &CanvasSession) -> Option<Instant> {
        let due = session.touch_deadline()?;
        Some(self.clock + Duration::from_secs_f64((due / 1000.0).max(0.0)))
    }

    /// Runs the touches' waits that have run out. True if anything happened.
    pub fn tick(&mut self, session: &CanvasSession) -> bool {
        let out = session.touch_tick(self.now_ms());
        self.take(out)
    }

    fn take(&mut self, out: Vec<TouchCommand>) -> bool {
        let any = !out.is_empty();
        self.commands.extend(out);
        any
    }

    /// What the mouse pointer is now: a tablet's pen moving it (X11 gives a pen as the mouse), as
    /// a pen or its eraser end with its pressure; else the mouse.
    fn mouse_kind() -> (PointerKind, f32) {
        match crate::pen::mouse_pen() {
            Some(pen) => (if pen.eraser { PointerKind::StylusEraser } else { PointerKind::Stylus }, pen.pressure),
            None => (PointerKind::Mouse, 1.0),
        }
    }

    /// A sample of pointer `id` at `(x, y)`: a pen's with its lean, if the system gave one.
    fn sample(&self, id: u64, (x, y): (f32, f32), pressure: f32) -> Vec<PenSample> {
        let (tilt, orientation) = crate::pen::lean(id).unwrap_or((0.0, 0.0));
        let roll = crate::pen::roll(id).unwrap_or(0.0);
        vec![PenSample { x, y, pressure: pressure.clamp(0.0, 1.0), tilt, orientation, time_ms: self.now_ms(), roll }]
    }

    fn pressed(&mut self, session: &CanvasSession, id: u64, kind: PointerKind, at: (f32, f32), pressure: f32) {
        self.down.retain(|d| *d != id);
        self.down.push(id);
        let out = session.touch_down(id, kind, self.sample(id, at, pressure));
        self.take(out);
    }

    fn moved(&mut self, session: &CanvasSession, id: u64, at: (f32, f32), pressure: f32) {
        let out = session.touch_move(id, self.sample(id, at, pressure));
        self.take(out);
    }

    fn lifted(&mut self, session: &CanvasSession, id: u64, at: (f32, f32), pressure: f32, cancelled: bool) {
        self.down.retain(|d| *d != id);
        let out = if cancelled { session.touch_cancel() } else { session.touch_up(id, self.sample(id, at, pressure)) };
        self.take(out);
    }

    /// Acts on `event` (`consumed` when the interface took it; `over` tells whether a point is on
    /// the interface rather than the canvas). True if anything changed. Presses go by `over` alone:
    /// `consumed` is worked out from the last frame, so a press that comes straight after the
    /// pointer (or a finger) arrives from the interface would count as the interface's.
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
                self.command_key = if cfg!(target_os = "macos") { state.super_key() } else { state.control_key() };
                self.shift = state.shift_key();
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::Space) => {
                self.space = event.state == ElementState::Pressed && !consumed;
                if !self.space && self.panning && !self.down.contains(&MOUSE) {
                    self.panning = false;
                    self.pen_panning = None;
                }
                true
            }
            WindowEvent::Focused(false) => {
                // keys let go elsewhere never come back up here
                self.space = false;
                self.command_key = false;
                false
            }
            WindowEvent::CursorEntered { .. } | WindowEvent::CursorLeft { .. } => {
                self.inside = matches!(event, WindowEvent::CursorEntered { .. });
                self.hover_shown
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                self.inside = true;
                if self.pan_to(session, p) {
                    return true;
                }
                if self.down.contains(&MOUSE) {
                    self.moved(session, MOUSE, p, Self::mouse_kind().1);
                    return true;
                }
                self.hover_shown
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(p) = self.cursor else { return false };
                match (button, state) {
                    (MouseButton::Left | MouseButton::Middle, ElementState::Pressed)
                        if self.down.is_empty() && !over(p) && (self.space || *button == MouseButton::Middle) =>
                    {
                        self.panning = true;
                        false
                    }
                    (MouseButton::Left, ElementState::Pressed) if !self.panning && !over(p) => {
                        let (kind, pressure) = Self::mouse_kind();
                        self.pressed(session, MOUSE, kind, p, pressure);
                        true
                    }
                    (MouseButton::Left, ElementState::Released) if self.down.contains(&MOUSE) => {
                        self.lifted(session, MOUSE, p, Self::mouse_kind().1, false);
                        true
                    }
                    (MouseButton::Left | MouseButton::Middle, ElementState::Released) => {
                        self.panning = false;
                        false
                    }
                    _ => false,
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(p) = self.cursor.filter(|p| !consumed && !over(*p)) else { return false };
                // with Shift the wheel turns the canvas about the pointer, up clockwise (the
                // desktop's own, for a mouse or a pen without touch; touch and trackpads turn it
                // as on the tablet); some systems send Shift's wheel sideways
                if self.shift {
                    let turn = match delta {
                        MouseScrollDelta::LineDelta(x, y) => (if *y == 0.0 { *x } else { *y }) * WHEEL_TURN,
                        MouseScrollDelta::PixelDelta(d) => (if d.y == 0.0 { d.x } else { d.y }) as f32 * SCROLL_TURN,
                    };
                    session.view_gesture(p.0, p.1, 0.0, 0.0, 1.0, turn);
                    return true;
                }
                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        session.view_gesture(p.0, p.1, 0.0, 0.0, WHEEL_ZOOM.powf(*y), 0.0);
                    }
                    MouseScrollDelta::PixelDelta(d) if self.command_key => {
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
            WindowEvent::Touch(touch) => self.on_touch(session, touch, &over),
            _ => false,
        }
    }

    /// The pointer at `p`: while the view is being moved, moved by as much. True if it was.
    fn pan_to(&mut self, session: &CanvasSession, p: (f32, f32)) -> bool {
        let last = self.cursor.replace(p);
        let (true, Some(l)) = (self.panning, last) else { return false };
        if self.turns_model && !self.shift {
            session.turn_model(p.0 - l.0, p.1 - l.1);
        } else {
            session.view_gesture(p.0, p.1, p.0 - l.0, p.1 - l.1, 1.0, 0.0);
        }
        true
    }

    fn on_touch(&mut self, session: &CanvasSession, touch: &Touch, over: &impl Fn((f32, f32)) -> bool) -> bool {
        let p = (touch.location.x as f32, touch.location.y as f32);
        // a pen with its pressure, or a finger: as the system says which (a touch screen may give
        // its fingers a pressure too), else a pressure means a pen
        let force = touch.force.map(|f| f.normalized() as f32);
        let pen = match crate::pen::is_pen(touch.id) {
            Some(true) => Some(force.unwrap_or(1.0)),
            Some(false) => None,
            None => force,
        };
        // a pen turned over rubs out with its eraser end
        let kind = match pen {
            Some(_) if crate::pen::eraser_end(touch.id) => PointerKind::StylusEraser,
            Some(_) => PointerKind::Stylus,
            None => PointerKind::Finger,
        };
        let pressure = pen.unwrap_or(1.0);
        // with space held the pen moves the view, as the mouse does
        if pen.is_some() {
            match touch.phase {
                TouchPhase::Started if self.space && self.down.is_empty() && !self.panning && !over(p) => {
                    self.panning = true;
                    self.pen_panning = Some(touch.id);
                    self.cursor = Some(p);
                    return false;
                }
                _ if self.pen_panning != Some(touch.id) => {}
                TouchPhase::Moved => return self.pan_to(session, p),
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    self.panning = false;
                    self.pen_panning = None;
                    return false;
                }
                TouchPhase::Started => {}
            }
        }
        let known = self.down.contains(&touch.id);
        match touch.phase {
            TouchPhase::Started => {
                // the first touch must land on the canvas; more may land anywhere
                if self.down.is_empty() && over(p) {
                    return false;
                }
                self.pressed(session, touch.id, kind, p, pressure);
                true
            }
            TouchPhase::Moved if known => {
                self.moved(session, touch.id, p, pressure);
                true
            }
            TouchPhase::Ended if known => {
                self.lifted(session, touch.id, p, pressure, false);
                true
            }
            TouchPhase::Cancelled if known => {
                self.lifted(session, touch.id, p, pressure, true);
                true
            }
            _ => false,
        }
    }
}
