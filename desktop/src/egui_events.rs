//! The window's events as egui takes them, where egui-winit can't make them (HarmonyOS, which has
//! no winit window: `ohos.rs`): the same translation egui-winit makes of winit's events (the first
//! touch down is egui's pointer, as a mouse), and keys from the system's keys by name, with the
//! text a US keyboard types with them and the clipboard's shortcuts on the app's own clipboard.
// (built everywhere, so its tests run on the desktop too)
#![cfg_attr(not(target_env = "ohos"), allow(dead_code))]

use winit::event::{ElementState, Force, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};

/// What egui wanted as of its last frame, which decides whether it takes an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wants {
    /// The pointer is over egui or dragging something of it.
    pub pointer: bool,
    /// A drag of egui's is under way.
    pub using_pointer: bool,
    /// A text field of egui's has the keys.
    pub keyboard: bool,
}

/// egui's input, as the window's events build it up between frames.
#[derive(Default)]
pub struct EguiEvents {
    pub input: egui::RawInput,
    pub modifiers: egui::Modifiers,
    /// The touch egui's pointer follows: the first down, until it lifts.
    pointer_touch: Option<u64>,
    /// Where egui's pointer is, points.
    pointer: Option<egui::Pos2>,
    /// Whether Ctrl+X, C and V cut, copy and paste (else they are plain keys, for the canvas's own
    /// shortcuts).
    pub clipboard_shortcuts: bool,
    /// The app's own clipboard: what egui last copied or cut.
    pub clipboard: String,
}

impl EguiEvents {
    pub fn new() -> EguiEvents {
        EguiEvents { clipboard_shortcuts: true, ..EguiEvents::default() }
    }

    /// Takes `event` in (positions in pixels, `pixels_per_point` the interface's: the window's scale
    /// times egui's zoom); true if egui takes it, as `wants` says (egui-winit's `consumed`).
    pub fn window_event(&mut self, event: &WindowEvent, pixels_per_point: f32, wants: Wants) -> bool {
        let ppp = pixels_per_point.max(f32::EPSILON);
        match event {
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
                    Some(*scale_factor as f32);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.button(*state == ElementState::Pressed, *button);
                wants.pointer
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(*x, *y)),
                    MouseScrollDelta::PixelDelta(p) => {
                        (egui::MouseWheelUnit::Point, egui::vec2(p.x as f32, p.y as f32) / ppp)
                    }
                };
                self.input.events.push(egui::Event::MouseWheel {
                    unit,
                    delta,
                    phase: touch_phase(*phase),
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: self.modifiers,
                });
                wants.pointer
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.moved(egui::pos2(position.x as f32, position.y as f32) / ppp);
                wants.using_pointer
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.input.events.push(egui::Event::PointerGone);
                false
            }
            WindowEvent::Touch(touch) => {
                self.touch(touch, ppp);
                match touch.phase {
                    TouchPhase::Moved => wants.using_pointer,
                    _ => wants.pointer,
                }
            }
            WindowEvent::Focused(focused) => {
                self.input.focused = *focused;
                if !focused {
                    self.modifiers = egui::Modifiers::default();
                }
                self.input.events.push(egui::Event::WindowFocused(*focused));
                false
            }
            WindowEvent::ModifiersChanged(state) => {
                let state = state.state();
                let ctrl = state.control_key();
                self.modifiers = egui::Modifiers {
                    alt: state.alt_key(),
                    ctrl,
                    shift: state.shift_key(),
                    mac_cmd: false,
                    command: ctrl,
                };
                self.input.events.push(egui::Event::ModifiersChanged(self.modifiers));
                false
            }
            _ => false,
        }
    }

    /// A key going down (`pressed`) or up: the clipboard's shortcuts (with the clipboard on) as
    /// cut, copy and paste, else the key, and pressed without Ctrl the text it types. True if egui
    /// takes it.
    pub fn key(&mut self, key: egui::Key, pressed: bool, repeat: bool, wants: Wants) -> bool {
        let command = self.modifiers.command;
        if pressed && self.clipboard_shortcuts && command {
            let shortcut = match key {
                egui::Key::X => Some(egui::Event::Cut),
                egui::Key::C => Some(egui::Event::Copy),
                egui::Key::V => (!self.clipboard.is_empty()).then(|| egui::Event::Paste(self.clipboard.clone())),
                _ => None,
            };
            if matches!(key, egui::Key::X | egui::Key::C | egui::Key::V) {
                self.input.events.extend(shortcut);
                return wants.keyboard;
            }
        }
        self.input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat,
            modifiers: self.modifiers,
        });
        if pressed
            && !command
            && !self.modifiers.ctrl
            && let Some(text) = key_text(key, self.modifiers.shift)
        {
            self.input.events.push(egui::Event::Text(text.to_string()));
        }
        wants.keyboard || key == egui::Key::Tab
    }

    /// What egui asked the system for after a frame: what it copied, kept as the app's own
    /// clipboard.
    pub fn commands(&mut self, commands: Vec<egui::OutputCommand>) {
        for command in commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.clipboard = text;
            }
        }
    }

    /// The input built up since the last frame, for a window of `size_px` pixels at
    /// `pixels_per_point` (and `scale`, the window's own), at `time` seconds.
    pub fn take(&mut self, size_px: (u32, u32), pixels_per_point: f32, scale: f32, time: f64) -> egui::RawInput {
        self.input.time = Some(time);
        let points = egui::vec2(size_px.0 as f32, size_px.1 as f32) / pixels_per_point.max(f32::EPSILON);
        self.input.screen_rect =
            (points.x > 0.0 && points.y > 0.0).then(|| egui::Rect::from_min_size(egui::Pos2::ZERO, points));
        self.input.viewport_id = egui::ViewportId::ROOT;
        self.input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(scale);
        self.input.take()
    }

    fn moved(&mut self, at: egui::Pos2) {
        self.pointer = Some(at);
        self.input.events.push(egui::Event::PointerMoved(at));
    }

    fn button(&mut self, pressed: bool, button: MouseButton) {
        let button = match button {
            MouseButton::Left => egui::PointerButton::Primary,
            MouseButton::Right => egui::PointerButton::Secondary,
            MouseButton::Middle => egui::PointerButton::Middle,
            MouseButton::Back => egui::PointerButton::Extra1,
            MouseButton::Forward => egui::PointerButton::Extra2,
            MouseButton::Other(_) => return,
        };
        if let Some(pos) = self.pointer {
            self.input.events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: self.modifiers });
        }
    }

    /// A touch: egui's own, and while it is the first down, the pointer (pressed as it lands,
    /// lifted and gone as it lifts, gone as it is cancelled).
    fn touch(&mut self, touch: &Touch, ppp: f32) {
        let pos = egui::pos2(touch.location.x as f32, touch.location.y as f32) / ppp;
        self.input.events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId::from(touch.id),
            phase: touch_phase(touch.phase),
            pos,
            force: touch.force.map(|f| match f {
                Force::Normalized(f) => f as f32,
                Force::Calibrated { force, max_possible_force, .. } => (force / max_possible_force) as f32,
            }),
        });
        if self.pointer_touch.is_some_and(|id| id != touch.id) {
            return;
        }
        match touch.phase {
            TouchPhase::Started => {
                self.pointer_touch = Some(touch.id);
                self.moved(pos);
                self.button(true, MouseButton::Left);
            }
            TouchPhase::Moved => self.moved(pos),
            TouchPhase::Ended => {
                self.pointer_touch = None;
                self.button(false, MouseButton::Left);
                self.pointer = None;
                self.input.events.push(egui::Event::PointerGone);
            }
            TouchPhase::Cancelled => {
                self.pointer_touch = None;
                self.pointer = None;
                self.input.events.push(egui::Event::PointerGone);
            }
        }
    }
}

fn touch_phase(phase: TouchPhase) -> egui::TouchPhase {
    match phase {
        TouchPhase::Started => egui::TouchPhase::Start,
        TouchPhase::Moved => egui::TouchPhase::Move,
        TouchPhase::Ended => egui::TouchPhase::End,
        TouchPhase::Cancelled => egui::TouchPhase::Cancel,
    }
}

/// The character `key` types on a US keyboard, with Shift held or not; none for a key that types
/// nothing.
pub fn key_text(key: egui::Key, shift: bool) -> Option<char> {
    use egui::Key as K;
    let name = key.name();
    let (plain, shifted) = match key {
        K::Space => (' ', ' '),
        K::Num0 => ('0', ')'),
        K::Num1 => ('1', '!'),
        K::Num2 => ('2', '@'),
        K::Num3 => ('3', '#'),
        K::Num4 => ('4', '$'),
        K::Num5 => ('5', '%'),
        K::Num6 => ('6', '^'),
        K::Num7 => ('7', '&'),
        K::Num8 => ('8', '*'),
        K::Num9 => ('9', '('),
        K::Minus => ('-', '_'),
        K::Equals => ('=', '+'),
        K::Plus => ('+', '+'),
        K::OpenBracket => ('[', '{'),
        K::CloseBracket => (']', '}'),
        K::Backslash => ('\\', '|'),
        K::Semicolon => (';', ':'),
        K::Quote => ('\'', '"'),
        K::Comma => (',', '<'),
        K::Period => ('.', '>'),
        K::Slash => ('/', '?'),
        K::Backtick => ('`', '~'),
        // a letter: its name is the capital
        _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase()) => {
            let capital = name.chars().next()?;
            (capital.to_ascii_lowercase(), capital)
        }
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalPosition;
    use winit::event::DeviceId;

    use super::*;

    const ALL: Wants = Wants { pointer: true, using_pointer: true, keyboard: true };

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> WindowEvent {
        WindowEvent::Touch(Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: Some(Force::Normalized(0.5)),
            id,
        })
    }

    #[test]
    fn the_first_touch_is_the_pointer_and_the_second_only_a_touch() {
        let mut egui = EguiEvents::new();
        egui.window_event(&touch(1, TouchPhase::Started, 200.0, 100.0), 2.0, ALL);
        egui.window_event(&touch(2, TouchPhase::Started, 400.0, 100.0), 2.0, ALL);
        egui.window_event(&touch(2, TouchPhase::Moved, 410.0, 100.0), 2.0, ALL);
        egui.window_event(&touch(1, TouchPhase::Ended, 220.0, 100.0), 2.0, ALL);
        let events = egui.take((800, 600), 2.0, 2.0, 0.0).events;
        let at = egui::pos2(100.0, 50.0);
        let pressed = egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(events.get(1), Some(&egui::Event::PointerMoved(at)), "{events:?}");
        assert_eq!(events.get(2), Some(&pressed));
        // the second touch's move is egui's touch alone
        let moves = events.iter().filter(|e| matches!(e, egui::Event::PointerMoved(_))).count();
        assert_eq!(moves, 1, "the first's landing alone: {events:?}");
        let force = events.iter().find_map(|e| match e {
            egui::Event::Touch { id, force, .. } if *id == egui::TouchId(2) => Some(*force),
            _ => None,
        });
        assert_eq!(force, Some(Some(0.5)));
        assert_eq!(events.last(), Some(&egui::Event::PointerGone));
        assert!(events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. })));
    }

    #[test]
    fn a_cancelled_touch_lets_the_pointer_go_unpressed() {
        let mut egui = EguiEvents::new();
        egui.window_event(&touch(5, TouchPhase::Started, 10.0, 10.0), 1.0, ALL);
        egui.window_event(&touch(5, TouchPhase::Cancelled, 10.0, 10.0), 1.0, ALL);
        let events = egui.take((100, 100), 1.0, 1.0, 0.0).events;
        assert_eq!(events.last(), Some(&egui::Event::PointerGone));
        assert!(!events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. })));
        // and the next touch is the pointer again
        egui.window_event(&touch(6, TouchPhase::Started, 20.0, 20.0), 1.0, ALL);
        let events = egui.take((100, 100), 1.0, 1.0, 0.0).events;
        assert!(events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: true, .. })));
    }

    #[test]
    fn a_touch_moving_is_egui_s_only_while_it_drags() {
        let mut egui = EguiEvents::new();
        let idle = Wants { pointer: true, ..Wants::default() };
        assert!(egui.window_event(&touch(1, TouchPhase::Started, 1.0, 1.0), 1.0, idle));
        assert!(!egui.window_event(&touch(1, TouchPhase::Moved, 2.0, 1.0), 1.0, idle));
        assert!(egui.window_event(&touch(1, TouchPhase::Moved, 3.0, 1.0), 1.0, ALL));
    }

    #[test]
    fn keys_type_their_text_and_copy_and_paste_on_the_app_s_clipboard() {
        let mut egui = EguiEvents::new();
        egui.key(egui::Key::A, true, false, ALL);
        egui.key(egui::Key::A, false, false, ALL);
        let events = egui.take((10, 10), 1.0, 1.0, 0.0).events;
        assert!(events.contains(&egui::Event::Text("a".to_owned())), "{events:?}");
        assert_eq!(events.iter().filter(|e| matches!(e, egui::Event::Text(_))).count(), 1, "only as it goes down");
        egui.commands(vec![egui::OutputCommand::CopyText("brush".to_owned())]);
        egui.modifiers = egui::Modifiers::COMMAND;
        egui.key(egui::Key::V, true, false, ALL);
        egui.key(egui::Key::C, true, false, ALL);
        let events = egui.take((10, 10), 1.0, 1.0, 0.0).events;
        assert_eq!(events, vec![egui::Event::Paste("brush".to_owned()), egui::Event::Copy]);
        // with the clipboard's shortcuts off (the canvas's own), Ctrl+C is a key
        egui.clipboard_shortcuts = false;
        egui.key(egui::Key::C, true, false, ALL);
        let events = egui.take((10, 10), 1.0, 1.0, 0.0).events;
        assert!(matches!(events.as_slice(), [egui::Event::Key { key: egui::Key::C, pressed: true, .. }]), "{events:?}");
    }

    #[test]
    fn shift_types_capitals_and_symbols() {
        assert_eq!(key_text(egui::Key::Q, false), Some('q'));
        assert_eq!(key_text(egui::Key::Q, true), Some('Q'));
        assert_eq!(key_text(egui::Key::Num2, true), Some('@'));
        assert_eq!(key_text(egui::Key::Slash, true), Some('?'));
        assert_eq!(key_text(egui::Key::Enter, false), None);
        assert_eq!(key_text(egui::Key::F1, false), None);
    }

    #[test]
    fn the_screen_is_in_points() {
        let mut egui = EguiEvents::new();
        let input = egui.take((2000, 1200), 2.0, 2.0, 1.5);
        assert_eq!(input.screen_rect, Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))));
        assert_eq!(input.time, Some(1.5));
    }
}
