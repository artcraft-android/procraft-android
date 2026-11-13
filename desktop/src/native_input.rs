//! The input of HarmonyOS's canvas component (its touches with their tool, pressure and tilt, the
//! mouse, keys) as the window events the app takes on the desktop: winit's, made as the Wayland
//! pen's are (`pen.rs`), a pen's kind, lean and eraser end kept for its touches as Windows's are.
//! Plain data, which the host fills in from the system's events (harmony/bridge).
// (built everywhere, so its tests run on the desktop too)
#![cfg_attr(not(target_env = "ohos"), allow(dead_code))]

use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, Force, MouseButton, Touch, TouchPhase, WindowEvent};
use winit::keyboard::ModifiersState;

/// Where a touch is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// What touches: the system says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Finger,
    Pen,
    /// A pen's eraser end.
    Eraser,
    Other,
}

/// A touch on the canvas component: where (pixels from its top left), its tool, its pressure
/// (0..1) and a pen's tilts along the panel (degrees, towards the right and towards the user).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeTouch {
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub phase: Phase,
    pub tool: Tool,
    pub force: f32,
    pub tilt: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    None,
}

/// The mouse on the canvas component: where (pixels), what it did, and with which button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeMouse {
    pub x: f32,
    pub y: f32,
    pub action: MouseAction,
    pub button: NativeButton,
}

/// A key, by what it is: one of egui's, or a modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeKey {
    Key(egui::Key),
    Shift,
    Control,
    Alt,
    Meta,
}

/// The id a touch goes by among the app's pointers (the mouse is `u64::MAX`).
pub fn touch_id(id: i32) -> u64 {
    u64::from(id.unsigned_abs())
}

/// `touch` as the window event the app takes: a pen's with its pressure as the force, its kind,
/// lean and eraser end kept for the strokes (`pen.rs`); a finger's with no force.
pub fn touch_event(touch: &NativeTouch) -> WindowEvent {
    let id = touch_id(touch.id);
    let pen = matches!(touch.tool, Tool::Pen | Tool::Eraser);
    crate::pen::pointer_kind(id, pen);
    if pen {
        let (tilt_x, tilt_y) = touch.tilt.unwrap_or((0.0, 0.0));
        crate::pen::keep_pose(id, crate::pen::lean_of(tilt_x, tilt_y), None);
        crate::pen::pen_flags(id, if touch.tool == Tool::Eraser { crate::pen::FLAG_ERASER } else { 0 });
    }
    let phase = match touch.phase {
        Phase::Down => TouchPhase::Started,
        Phase::Move => TouchPhase::Moved,
        Phase::Up => TouchPhase::Ended,
        Phase::Cancel => TouchPhase::Cancelled,
    };
    let force = pen
        .then(|| Force::Normalized(f64::from(if touch.force.is_finite() { touch.force.clamp(0.0, 1.0) } else { 0.0 })));
    WindowEvent::Touch(Touch {
        device_id: DeviceId::dummy(),
        phase,
        location: PhysicalPosition::new(f64::from(touch.x), f64::from(touch.y)),
        force,
        id,
    })
}

/// `mouse` as the window events the app takes: the pointer moving there, then its button.
pub fn mouse_events(mouse: &NativeMouse) -> Vec<WindowEvent> {
    let device_id = DeviceId::dummy();
    let mut events = vec![WindowEvent::CursorMoved {
        device_id,
        position: PhysicalPosition::new(f64::from(mouse.x), f64::from(mouse.y)),
    }];
    let button = match mouse.button {
        NativeButton::Left => MouseButton::Left,
        NativeButton::Right => MouseButton::Right,
        NativeButton::Middle => MouseButton::Middle,
        NativeButton::Back => MouseButton::Back,
        NativeButton::Forward => MouseButton::Forward,
        NativeButton::None => return events,
    };
    let state = match mouse.action {
        MouseAction::Press => ElementState::Pressed,
        MouseAction::Release => ElementState::Released,
        MouseAction::Move => return events,
    };
    events.push(WindowEvent::MouseInput { device_id, state, button });
    events
}

/// The modifier keys held, as keys go down and up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    shift: bool,
    control: bool,
    alt: bool,
    meta: bool,
}

impl Held {
    /// `key` went down (`pressed`) or up: the modifiers' window event if they changed.
    pub fn key(&mut self, key: NativeKey, pressed: bool) -> Option<WindowEvent> {
        let before = *self;
        match key {
            NativeKey::Shift => self.shift = pressed,
            NativeKey::Control => self.control = pressed,
            NativeKey::Alt => self.alt = pressed,
            NativeKey::Meta => self.meta = pressed,
            NativeKey::Key(_) => {}
        }
        (*self != before).then(|| WindowEvent::ModifiersChanged(self.state().into()))
    }

    fn state(self) -> ModifiersState {
        let mut state = ModifiersState::empty();
        state.set(ModifiersState::SHIFT, self.shift);
        state.set(ModifiersState::CONTROL, self.control);
        state.set(ModifiersState::ALT, self.alt);
        state.set(ModifiersState::SUPER, self.meta);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen(id: i32, phase: Phase, tool: Tool, tilt: Option<(f32, f32)>) -> NativeTouch {
        NativeTouch { id, x: 30.0, y: 40.0, phase, tool, force: 0.7, tilt }
    }

    #[test]
    fn a_pen_s_touch_carries_its_pressure_and_keeps_its_kind_and_lean() {
        let event = touch_event(&pen(901, Phase::Down, Tool::Pen, Some((45.0, 0.0))));
        let WindowEvent::Touch(touch) = event else { panic!("{event:?}") };
        assert_eq!(touch.id, 901);
        assert_eq!(touch.phase, TouchPhase::Started);
        assert_eq!(touch.location, PhysicalPosition::new(30.0, 40.0));
        let force = touch.force.map(|f| f.normalized());
        assert!(force.is_some_and(|f| (f - 0.7).abs() < 1e-6), "{force:?}");
        assert_eq!(crate::pen::is_pen(901), Some(true));
        assert!(!crate::pen::eraser_end(901));
        // leaning 45 degrees to the right
        let (tilt, towards) = crate::pen::lean(901).expect("a lean kept");
        assert!(
            (tilt - std::f32::consts::FRAC_PI_4).abs() < 1e-4 && (towards - std::f32::consts::FRAC_PI_2).abs() < 1e-4
        );
    }

    #[test]
    fn an_eraser_end_and_a_finger_go_as_such() {
        touch_event(&pen(902, Phase::Down, Tool::Eraser, None));
        assert!(crate::pen::eraser_end(902));
        let event = touch_event(&NativeTouch {
            id: 903,
            x: 1.0,
            y: 2.0,
            phase: Phase::Up,
            tool: Tool::Finger,
            force: 0.3,
            tilt: None,
        });
        let WindowEvent::Touch(touch) = event else { panic!("{event:?}") };
        assert_eq!(touch.force, None, "a finger's pressure isn't a pen's");
        assert_eq!(touch.phase, TouchPhase::Ended);
        assert_eq!(crate::pen::is_pen(903), Some(false));
    }

    #[test]
    fn the_mouse_moves_then_presses() {
        let press = NativeMouse { x: 5.0, y: 6.0, action: MouseAction::Press, button: NativeButton::Left };
        let events = mouse_events(&press);
        assert!(
            matches!(
                events.as_slice(),
                [
                    WindowEvent::CursorMoved { .. },
                    WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }
                ]
            ),
            "{events:?}"
        );
        let moving = NativeMouse { action: MouseAction::Move, button: NativeButton::None, ..press };
        assert_eq!(mouse_events(&moving).len(), 1);
    }

    #[test]
    fn modifiers_change_as_they_go_down_and_up() {
        let mut held = Held::default();
        let Some(WindowEvent::ModifiersChanged(m)) = held.key(NativeKey::Control, true) else { panic!("no change") };
        assert!(m.state().control_key() && !m.state().shift_key());
        assert_eq!(held.key(NativeKey::Control, true), None, "held still");
        assert_eq!(held.key(NativeKey::Key(egui::Key::A), true), None);
        let Some(WindowEvent::ModifiersChanged(m)) = held.key(NativeKey::Control, false) else { panic!("no change") };
        assert!(m.state().is_empty());
    }
}
