//! The window's pointers handed to the session's touches (crates/core's `touch`, shared with the
//! Android app), which paint, move the canvas and say what the app should do. The mouse adds what
//! it can't do by touch: the wheel zooms, and the middle button (or space) moves the canvas; on a
//! model it turns it, as a finger does, and with Shift moves it. Dragged with Space and Ctrl (⌘)
//! held it zooms the canvas, with R held it turns it (with Shift in 15° steps), and R tapped turns
//! it a step (the core's key drags, keys.rs). A pen and fingers go on as they do.

use std::time::{Duration, Instant};

use procraft_core::{CanvasSession, KeyDrag, PenSample, PointerKind, TouchCommand};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

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

/// R let go within this, with no drag, is a tap: it turns the canvas a step (the touches' tap).
const R_TAP: Duration = Duration::from_millis(300);

/// The mouse's pointer id: touches have the platform's.
const MOUSE: u64 = u64::MAX;

pub struct Input {
    clock: Instant,
    /// What the touches asked, for the app to do at the next frame.
    pub commands: Vec<TouchCommand>,
    /// The pointers down on the canvas.
    down: Vec<u64>,
    /// Pointers down that the canvas didn't take (pressed on the interface, or on the canvas
    /// while a popover was open, which that press only closes): while one is, no brush outline
    /// hovers (it would stay where the pen went down, as the pen moves on).
    ignored: Vec<u64>,
    /// The mouse: where it is, whether it is over the window, and whether it (or space) moves the
    /// view.
    cursor: Option<(f32, f32)>,
    inside: bool,
    panning: bool,
    /// The mouse dragged with a key held, zooming or turning the canvas (the core's key drag).
    key_drag: Option<KeyDrag>,
    /// The pen moving the view (pressed with space held), as the mouse does.
    pen_panning: Option<u64>,
    /// The brush cursor follows the mouse hovering, so its moves draw a frame.
    pub hover_shown: bool,
    space: bool,
    /// R held for the canvas (not typed) since when, and whether the mouse went down since (then
    /// it turned the canvas by dragging, and letting go turns it no step).
    r_key: Option<Instant>,
    r_dragged: bool,
    command_key: bool,
    /// Ctrl and Alt held (on a Mac, Space with Ctrl zooms as with ⌘: the system may keep either).
    control: bool,
    alt: bool,
    shift: bool,
    /// The interface takes the keys (text being edited, the hex keypad): R is typed, not a turn.
    pub keys_taken: bool,
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
            ignored: Vec::new(),
            cursor: None,
            inside: false,
            panning: false,
            key_drag: None,
            pen_panning: None,
            hover_shown: false,
            space: false,
            r_key: None,
            r_dragged: false,
            command_key: false,
            control: false,
            alt: false,
            shift: false,
            keys_taken: false,
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

    /// Whether the mouse is over the window.
    pub fn inside(&self) -> bool {
        self.inside
    }

    pub fn hovering(&self) -> Option<(f32, f32)> {
        self.cursor.filter(|_| self.inside && self.down.is_empty() && self.ignored.is_empty() && !self.moving_view())
    }

    /// Nothing is pressed on the canvas and the view isn't moving.
    pub fn idle(&self) -> bool {
        self.down.is_empty() && !self.panning && self.key_drag.is_none()
    }

    /// Space or R is held, or the mouse moves the view.
    pub fn moving_view(&self) -> bool {
        self.space || self.r_key.is_some() || self.panning || self.key_drag.is_some()
    }

    /// The mouse zooms the view, or would if pressed.
    pub fn zooming_view(&self) -> bool {
        self.key_drag.or_else(|| self.drag_held()) == Some(KeyDrag::Zoom)
    }

    /// The drag the keys held make of the mouse pressed on the canvas: R turns it, Space with
    /// Ctrl (⌘) zooms it (Space alone moves it, as the middle button does).
    fn drag_held(&self) -> Option<KeyDrag> {
        if self.r_key.is_some() {
            Some(KeyDrag::Turn)
        } else if self.space && (self.command_key || self.control) {
            Some(KeyDrag::Zoom)
        } else {
            None
        }
    }

    fn end_key_drag(&mut self, session: &CanvasSession) {
        if self.key_drag.take().is_some() {
            session.key_drag_end();
        }
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
                self.control = state.control_key();
                self.alt = state.alt_key();
                let shift = std::mem::replace(&mut self.shift, state.shift_key());
                // a turn goes onto its steps as Shift goes down, and off them as it comes up
                if let (Some(KeyDrag::Turn), Some(p), true) = (self.key_drag, self.cursor, shift != self.shift) {
                    session.key_drag_move(p.0, p.1, self.shift);
                    return true;
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::Space) => {
                self.space = event.state == ElementState::Pressed && !consumed;
                if !self.space && self.panning && !self.down.contains(&MOUSE) {
                    self.panning = false;
                    self.pen_panning = None;
                }
                // a zoom ends with it too
                if !self.space && self.key_drag == Some(KeyDrag::Zoom) {
                    self.end_key_drag(session);
                }
                true
            }
            WindowEvent::KeyboardInput { event, is_synthetic, .. } if is_r(event) => {
                match event.state {
                    // (a press as the window comes to the front is a key held from before: no tap)
                    ElementState::Pressed if !event.repeat && !*is_synthetic => {
                        let typed = consumed || self.keys_taken || self.command_key || self.control || self.alt;
                        self.r_key = (!typed).then(Instant::now);
                        self.r_dragged = false;
                    }
                    ElementState::Pressed => {}
                    ElementState::Released => {
                        // tapped, it turns the canvas a step (with Shift back); a drag turned it
                        let tapped = self.r_key.take().is_some_and(|since| since.elapsed() < R_TAP);
                        if tapped && !self.r_dragged && !consumed {
                            session.key_turn(self.shift);
                        }
                        if self.key_drag == Some(KeyDrag::Turn) {
                            self.end_key_drag(session);
                        }
                    }
                }
                true
            }
            WindowEvent::Focused(false) => {
                // keys let go elsewhere never come back up here (nor, perhaps, a press's lift)
                self.space = false;
                self.r_key = None;
                self.command_key = false;
                self.control = false;
                self.alt = false;
                self.shift = false;
                self.end_key_drag(session);
                self.ignored.clear();
                false
            }
            WindowEvent::CursorEntered { .. } | WindowEvent::CursorLeft { .. } => {
                self.inside = matches!(event, WindowEvent::CursorEntered { .. });
                self.hover_shown
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                self.inside = true;
                if self.key_drag.is_some() {
                    self.cursor = Some(p);
                    session.key_drag_move(p.0, p.1, self.shift);
                    return true;
                }
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
                    // with R held it turns the canvas, with Space and Ctrl (⌘) zooms it: nothing
                    // is painted
                    (MouseButton::Left, ElementState::Pressed)
                        if self.down.is_empty() && !self.panning && !over(p) && self.drag_held().is_some() =>
                    {
                        if let Some(how) = self.drag_held() {
                            session.key_drag_begin(how, p.0, p.1);
                            self.key_drag = Some(how);
                            self.r_dragged |= how == KeyDrag::Turn;
                        }
                        false
                    }
                    (MouseButton::Left, ElementState::Released) if self.key_drag.is_some() => {
                        self.end_key_drag(session);
                        false
                    }
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
        // (a pen landing or lifting has no force, its pressure being nothing: its own is taken)
        let pen = match crate::pen::is_pen(touch.id) {
            Some(true) => Some(force.or_else(|| crate::pen::pressure(touch.id)).unwrap_or(1.0)),
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
                // the first touch must land on the canvas; more may land anywhere (one that didn't
                // is kept, so no outline hovers while it is down)
                if self.down.is_empty() && over(p) {
                    self.ignored.retain(|i| *i != touch.id);
                    self.ignored.push(touch.id);
                    return true;
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
            TouchPhase::Ended | TouchPhase::Cancelled if self.ignored.contains(&touch.id) => {
                self.ignored.retain(|i| *i != touch.id);
                true
            }
            _ => false,
        }
    }
}

/// Whether `event` is the R key's: by its letter, or (a layout without Latin letters) by its
/// place, as the interface's own shortcuts take a key.
fn is_r(event: &KeyEvent) -> bool {
    match &event.logical_key {
        Key::Character(c) if c.eq_ignore_ascii_case("r") => true,
        Key::Character(c) => {
            egui::Key::from_name(c).is_none() && event.physical_key == PhysicalKey::Code(KeyCode::KeyR)
        }
        _ => false,
    }
}
