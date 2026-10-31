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
/// A place in the window (pixels).
type Point = (f32, f32);

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
    /// The brush cursor follows the mouse hovering, so its moves draw a frame.
    pub hover_shown: bool,
    space: bool,
    command_key: bool,
    shift: bool,
    /// A model is shown (not its 2D texture): dragging the view turns it, as one finger does
    /// (which finger, and where it last was).
    pub turns_model: bool,
    turning: Option<(u64, (f32, f32))>,
    /// The touch companion's Zoom or Move on: a press dragged on the canvas zooms or moves it
    /// (which pointer, where it went down and where it last was); and the screen's density.
    pub companion: crate::companion::Mode,
    companion_drag: Option<(u64, Point, Point)>,
    pub pixels_per_point: f32,
    /// Recolor is on: a press on the canvas drags its reticle, or (a tap) fills from where it
    /// lands. The press (which pointer, where it went down and last was, whether it is a drag),
    /// how far the reticle has been dragged since the interface last looked, and a tap's place.
    pub recolor: bool,
    recolor_press: Option<(u64, Point, Point, bool)>,
    pub recolor_moved: Point,
    pub recolor_tap: Option<Point>,
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
            hover_shown: false,
            space: false,
            command_key: false,
            shift: false,
            turns_model: false,
            turning: None,
            companion: crate::companion::Mode::None,
            companion_drag: None,
            pixels_per_point: 1.0,
            recolor: false,
            recolor_press: None,
            recolor_moved: (0.0, 0.0),
            recolor_tap: None,
        }
    }
}

impl Input {
    /// Where the mouse hovers (window pixels): over the window, nothing pressed, the view not
    /// moving.
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

    /// A press going down at `at` with the companion's Zoom or Move on: the companion's, not the
    /// touches'. True if it took it.
    fn companion_down(&mut self, id: u64, at: (f32, f32)) -> bool {
        if self.companion == crate::companion::Mode::None || !self.down.is_empty() || self.companion_drag.is_some() {
            return false;
        }
        self.companion_drag = Some((id, at, at));
        true
    }

    /// The companion's press `id` moved to `at`: the canvas zooms (up zooming in, about where it
    /// went down) or moves. True if it was the companion's.
    fn companion_moved(&mut self, session: &CanvasSession, id: u64, at: (f32, f32)) -> bool {
        let Some((held, start, last)) = self.companion_drag.filter(|d| d.0 == id) else { return false };
        let (dx, dy) = (at.0 - last.0, at.1 - last.1);
        if self.companion == crate::companion::Mode::Zoom {
            let scale = (-dy / (crate::companion::ZOOM_DISTANCE * self.pixels_per_point)).exp();
            session.view_gesture(start.0, start.1, 0.0, 0.0, scale, 0.0);
        } else {
            session.view_gesture(at.0, at.1, dx, dy, 1.0, 0.0);
        }
        self.companion_drag = Some((held, start, at));
        true
    }

    /// A press going down at `at` with Recolor on: Recolor's. True if it took it.
    fn recolor_down(&mut self, id: u64, at: Point) -> bool {
        if !self.recolor || !self.down.is_empty() || self.recolor_press.is_some() {
            return false;
        }
        self.recolor_press = Some((id, at, at, false));
        true
    }

    /// Recolor's press `id` moved to `at`: past a tap's reach, the reticle follows it. True if it
    /// was Recolor's.
    fn recolor_moved_to(&mut self, id: u64, at: Point) -> bool {
        let Some((held, start, last, dragging)) = self.recolor_press.filter(|p| p.0 == id) else { return false };
        let slop = 8.0 * self.pixels_per_point;
        let dragging = dragging || (at.0 - start.0).hypot(at.1 - start.1) > slop;
        if dragging {
            self.recolor_moved = (self.recolor_moved.0 + at.0 - last.0, self.recolor_moved.1 + at.1 - last.1);
        }
        self.recolor_press = Some((held, start, if dragging { at } else { last }, dragging));
        true
    }

    /// Recolor's press `id` let go at `at`: a tap fills from there. True if it was Recolor's.
    fn recolor_up(&mut self, id: u64, at: Point) -> bool {
        let Some((_, _, _, dragging)) = self.recolor_press.filter(|p| p.0 == id) else { return false };
        self.recolor_press = None;
        if !dragging {
            self.recolor_tap = Some(at);
        }
        true
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

    fn sample(&self, (x, y): (f32, f32), pressure: f32) -> Vec<PenSample> {
        vec![PenSample {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            tilt: 0.0,
            orientation: 0.0,
            time_ms: self.now_ms(),
        }]
    }

    fn pressed(&mut self, session: &CanvasSession, id: u64, kind: PointerKind, at: (f32, f32), pressure: f32) {
        self.down.retain(|d| *d != id);
        self.down.push(id);
        let out = session.touch_down(id, kind, self.sample(at, pressure));
        self.take(out);
    }

    fn moved(&mut self, session: &CanvasSession, id: u64, at: (f32, f32), pressure: f32) {
        let out = session.touch_move(id, self.sample(at, pressure));
        self.take(out);
    }

    fn lifted(&mut self, session: &CanvasSession, id: u64, at: (f32, f32), pressure: f32, cancelled: bool) {
        self.down.retain(|d| *d != id);
        let out = if cancelled { session.touch_cancel() } else { session.touch_up(id, self.sample(at, pressure)) };
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
                let last = self.cursor.replace(p);
                if let (true, Some(l)) = (self.panning, last) {
                    if self.turns_model && !self.shift {
                        session.turn_model(p.0 - l.0, p.1 - l.1);
                    } else {
                        session.view_gesture(p.0, p.1, p.0 - l.0, p.1 - l.1, 1.0, 0.0);
                    }
                    return true;
                }
                if self.companion_moved(session, MOUSE, p) || self.recolor_moved_to(MOUSE, p) {
                    return true;
                }
                if self.down.contains(&MOUSE) {
                    self.moved(session, MOUSE, p, 1.0);
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
                        if !self.companion_down(MOUSE, p) && !self.recolor_down(MOUSE, p) {
                            self.pressed(session, MOUSE, PointerKind::Mouse, p, 1.0);
                        }
                        true
                    }
                    (MouseButton::Left, ElementState::Released) if self.recolor_up(MOUSE, p) => true,
                    (MouseButton::Left, ElementState::Released)
                        if self.companion_drag.is_some_and(|d| d.0 == MOUSE) =>
                    {
                        self.companion_drag = None;
                        true
                    }
                    (MouseButton::Left, ElementState::Released) if self.down.contains(&MOUSE) => {
                        self.lifted(session, MOUSE, p, 1.0, false);
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

    fn on_touch(&mut self, session: &CanvasSession, touch: &Touch, over: &impl Fn((f32, f32)) -> bool) -> bool {
        let p = (touch.location.x as f32, touch.location.y as f32);
        // the platform gives a pen's pressure, and none for a finger
        let pen = touch.force.map(|f| f.normalized() as f32);
        let kind = if pen.is_some() { PointerKind::Stylus } else { PointerKind::Finger };
        let pressure = pen.unwrap_or(1.0);
        let known = self.down.contains(&touch.id);
        // the companion's Zoom or Move takes a press on the canvas
        match touch.phase {
            TouchPhase::Started if !over(p) && self.turning.is_none() && self.companion_down(touch.id, p) => {
                return true;
            }
            TouchPhase::Started if !over(p) && self.turning.is_none() && self.recolor_down(touch.id, p) => return true,
            TouchPhase::Moved if self.companion_moved(session, touch.id, p) || self.recolor_moved_to(touch.id, p) => {
                return true;
            }
            TouchPhase::Ended if self.recolor_up(touch.id, p) => return true,
            TouchPhase::Cancelled if self.recolor_press.is_some_and(|r| r.0 == touch.id) => {
                self.recolor_press = None;
                return true;
            }
            TouchPhase::Ended | TouchPhase::Cancelled if self.companion_drag.is_some_and(|d| d.0 == touch.id) => {
                self.companion_drag = None;
                return true;
            }
            _ => {}
        }
        // on a shown model one finger turns it; a second one hands both to the touches
        if let Some((id, last)) = self.turning {
            match touch.phase {
                TouchPhase::Moved if touch.id == id => {
                    session.turn_model(p.0 - last.0, p.1 - last.1);
                    self.turning = Some((id, p));
                    return true;
                }
                TouchPhase::Ended | TouchPhase::Cancelled if touch.id == id => {
                    self.turning = None;
                    return true;
                }
                TouchPhase::Started => {
                    self.turning = None;
                    self.pressed(session, id, PointerKind::Finger, last, 1.0);
                }
                _ => {}
            }
        }
        match touch.phase {
            TouchPhase::Started if self.turns_model && pen.is_none() && self.down.is_empty() && !over(p) => {
                self.turning = Some((touch.id, p));
                true
            }
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
