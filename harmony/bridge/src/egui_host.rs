//! The desktop app's egui interface in the canvas component, for now: when ArkUI loads this
//! library as an XComponent's native side (`libraryname: 'procraft_harmony'`), its surface, touches,
//! mouse, keys and frames go to `procraft_desktop::ohos`, which drives the app as winit does on
//! the desktop. ArkTS calls [`start_app`] first, with the app's folder and the display's density.
// Node-API's registration code, which the `napi` attribute writes, is unsafe; so is the system's
// log, called directly.
#![allow(unsafe_code)]
// (in a test build Node-API registers nothing, so none of this is called)
#![cfg_attr(test, allow(dead_code))]

use std::ffi::{CString, c_char, c_int, c_uint};
use std::ptr::NonNull;

use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::Object;
use napi_ohos::{Env, Result};
use ohos_xcomponent_binding::{
    Action, KeyCode, KeyEventData, MouseAction, MouseButton, MouseEventData, TouchEvent, TouchEventData,
    TouchPointTool, XComponent,
};
use procraft_desktop::egui::Key;
use procraft_desktop::ohos::{self, NativeButton, NativeKey, NativeMouse, NativeTouch, Phase, Tool};

/// Starts the app: `filesDir` is the app's sandbox folder (`context.filesDir`), `density` the
/// display's pixels per vp (`display.getDefaultDisplaySync().densityPixels`). Called by ArkTS
/// before the canvas component is made.
#[napi]
pub fn start_app(files_dir: String, density: f64) {
    ohos::log_to(hilog);
    ohos::start(files_dir.into(), density as f32);
}

/// The canvas component's native side: its callbacks hooked up to the app. Run as this library is
/// loaded, for ArkTS's import too, where there is no component (nothing is hooked up then).
#[napi(module_exports)]
pub fn init(exports: Object, env: Env) -> Result<()> {
    let Ok(component) = XComponent::init(env, exports) else { return Ok(()) };
    component.on_surface_created(|component, window| {
        let size = component.size(window)?;
        if let Some(native) = NonNull::new(window.0) {
            ohos::surface_created(native, pixels(size.width), pixels(size.height));
        }
        Ok(())
    });
    component.on_surface_changed(|component, window| {
        let size = component.size(window)?;
        ohos::surface_changed(pixels(size.width), pixels(size.height));
        Ok(())
    });
    component.on_surface_destroyed(|_, _| {
        ohos::surface_destroyed();
        Ok(())
    });
    component.on_touch_event(|_, _, event| {
        if let Some(touch) = touch(&event) {
            ohos::touch(touch);
        }
        Ok(())
    });
    component.on_mouse_event(|_, _, event| {
        ohos::mouse(mouse(&event));
        Ok(())
    })?;
    component.on_hover_event(|_, inside| {
        ohos::hover(inside);
        Ok(())
    })?;
    component.register_mouse_event_callback()?;
    component.on_key_event(|_, _, event| {
        key(&event);
        Ok(())
    })?;
    component.on_frame_callback(|_, _, _| {
        ohos::frame();
        Ok(())
    })?;
    component.register_callback()?;
    Ok(())
}

/// A size the system gives, as the app's pixels.
fn pixels(size: u64) -> u32 {
    u32::try_from(size).unwrap_or(u32::MAX)
}

/// The touch that changed in `event`, with its tool and tilt from the event's list of points (in
/// pixels from the component's top left: to be checked on a device, else times the density).
fn touch(event: &TouchEventData) -> Option<NativeTouch> {
    let phase = match event.event_type {
        TouchEvent::Down => Phase::Down,
        TouchEvent::Move => Phase::Move,
        TouchEvent::Up => Phase::Up,
        TouchEvent::Cancel => Phase::Cancel,
        TouchEvent::Unknown => return None,
    };
    let point = event.touch_points.iter().find(|p| p.id == event.id);
    let tool = match point.map(|p| p.event_tool_type) {
        Some(TouchPointTool::Pen | TouchPointTool::Pencil | TouchPointTool::Brush | TouchPointTool::Airbrush) => {
            Tool::Pen
        }
        Some(TouchPointTool::Rubber) => Tool::Eraser,
        Some(TouchPointTool::Finger) | None => Tool::Finger,
        Some(_) => Tool::Other,
    };
    let tilt = point.and_then(|p| Some((p.tilt_x?, p.tilt_y?)));
    Some(NativeTouch { id: event.id, x: event.x, y: event.y, phase, tool, force: event.force, tilt })
}

fn mouse(event: &MouseEventData) -> NativeMouse {
    let action = match event.action {
        MouseAction::Press => ohos::MouseAction::Press,
        MouseAction::Release => ohos::MouseAction::Release,
        _ => ohos::MouseAction::Move,
    };
    let button = match event.button {
        MouseButton::LeftButton => NativeButton::Left,
        MouseButton::RightButton => NativeButton::Right,
        MouseButton::MiddleButton => NativeButton::Middle,
        MouseButton::BackButton => NativeButton::Back,
        MouseButton::ForwardButton => NativeButton::Forward,
        MouseButton::NoneButton => NativeButton::None,
    };
    NativeMouse { x: event.x, y: event.y, action, button }
}

fn key(event: &KeyEventData) {
    let pressed = match event.action {
        Action::Down => true,
        Action::Up => false,
        Action::Unknown => return,
    };
    if let Some(key) = native_key(event.code) {
        ohos::key(key, pressed);
    }
}

/// A hardware keyboard's key as the app names it; none for one it has no use for.
fn native_key(code: KeyCode) -> Option<NativeKey> {
    use KeyCode as C;
    let key = match code {
        C::ShiftLeft | C::ShiftRight => return Some(NativeKey::Shift),
        C::CtrlLeft | C::CtrlRight => return Some(NativeKey::Control),
        C::AltLeft | C::AltRight => return Some(NativeKey::Alt),
        C::MetaLeft | C::MetaRight => return Some(NativeKey::Meta),
        C::A => Key::A,
        C::B => Key::B,
        C::C => Key::C,
        C::D => Key::D,
        C::E => Key::E,
        C::F => Key::F,
        C::G => Key::G,
        C::H => Key::H,
        C::I => Key::I,
        C::J => Key::J,
        C::K => Key::K,
        C::L => Key::L,
        C::M => Key::M,
        C::N => Key::N,
        C::O => Key::O,
        C::P => Key::P,
        C::Q => Key::Q,
        C::R => Key::R,
        C::S => Key::S,
        C::T => Key::T,
        C::U => Key::U,
        C::V => Key::V,
        C::W => Key::W,
        C::X => Key::X,
        C::Y => Key::Y,
        C::Z => Key::Z,
        C::Key0 | C::Numpad0 => Key::Num0,
        C::Key1 | C::Numpad1 => Key::Num1,
        C::Key2 | C::Numpad2 => Key::Num2,
        C::Key3 | C::Numpad3 => Key::Num3,
        C::Key4 | C::Numpad4 => Key::Num4,
        C::Key5 | C::Numpad5 => Key::Num5,
        C::Key6 | C::Numpad6 => Key::Num6,
        C::Key7 | C::Numpad7 => Key::Num7,
        C::Key8 | C::Numpad8 => Key::Num8,
        C::Key9 | C::Numpad9 => Key::Num9,
        C::Space => Key::Space,
        C::Enter => Key::Enter,
        C::Tab => Key::Tab,
        C::Del => Key::Backspace,
        C::ForwardDel => Key::Delete,
        C::Escape => Key::Escape,
        C::DpadUp => Key::ArrowUp,
        C::DpadDown => Key::ArrowDown,
        C::DpadLeft => Key::ArrowLeft,
        C::DpadRight => Key::ArrowRight,
        C::MoveHome => Key::Home,
        C::MoveEnd => Key::End,
        C::PageUp => Key::PageUp,
        C::PageDown => Key::PageDown,
        C::Insert => Key::Insert,
        C::Minus => Key::Minus,
        C::Equals => Key::Equals,
        C::Plus => Key::Plus,
        C::LeftBracket => Key::OpenBracket,
        C::RightBracket => Key::CloseBracket,
        C::Backslash => Key::Backslash,
        C::Semicolon => Key::Semicolon,
        C::Apostrophe => Key::Quote,
        C::Comma => Key::Comma,
        C::Period => Key::Period,
        C::Slash => Key::Slash,
        C::Grave => Key::Backtick,
        C::F1 => Key::F1,
        C::F2 => Key::F2,
        C::F3 => Key::F3,
        C::F4 => Key::F4,
        C::F5 => Key::F5,
        C::F6 => Key::F6,
        C::F7 => Key::F7,
        C::F8 => Key::F8,
        C::F9 => Key::F9,
        C::F10 => Key::F10,
        C::F11 => Key::F11,
        C::F12 => Key::F12,
        _ => return None,
    };
    Some(NativeKey::Key(key))
}

// The system's log (libhilog_ndk.z.so), with the line as its one public argument: written as the
// format itself, a `%` in it would be read as one.
#[link(name = "hilog_ndk.z")]
unsafe extern "C" {
    fn OH_LOG_Print(kind: c_int, level: c_int, domain: c_uint, tag: *const c_char, fmt: *const c_char, ...) -> c_int;
}

/// hilog's app log, its levels (hilog/log.h), the app's domain and tag (`hdc hilog | grep ProCraft`).
const LOG_APP: c_int = 0;
const LOG_INFO: c_int = 4;
const LOG_WARN: c_int = 5;
const LOG_ERROR: c_int = 6;
const DOMAIN: c_uint = 0x5043;

/// A line of the app's log, into hilog at the level it says.
fn hilog(line: &str) {
    let level = if line.contains(" ERROR ") {
        LOG_ERROR
    } else if line.contains(" WARN ") {
        LOG_WARN
    } else {
        LOG_INFO
    };
    let Ok(text) = CString::new(line.replace('\0', " ")) else { return };
    // SAFETY: nul-ended strings that outlive the call, and a format taking the one string given
    unsafe {
        OH_LOG_Print(LOG_APP, level, DOMAIN, c"ProCraft".as_ptr(), c"%{public}s".as_ptr(), text.as_ptr());
    }
}
