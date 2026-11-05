//! A pen's lean, its buttons and its eraser end, which winit leaves out of its touches: on Windows
//! read from the system's pointer messages as they reach the window (before winit makes its
//! touches of them), kept by pointer for the touches to carry. Under X11 and on a Mac winit gives a
//! tablet's pen as the mouse: its pressure, lean and eraser end are read from its own events
//! instead (XInput 2's; AppKit's tablet events), for the mouse's samples to carry. On Wayland winit
//! reads no pen at all (the compositor makes a mouse of it, without pressure), so the whole pen is
//! read from the tablet protocol and handed on as the touches and hovering a pen makes on Windows.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Each pen pointer's last lean: radians from upright, and the direction it leans (radians
/// clockwise from screen-up), as the strokes take them.
static LEANS: Mutex<Option<HashMap<u64, (f32, f32)>>> = Mutex::new(None);

/// Each pen pointer's last pressure (0..1), for a pen that gives one.
static PRESSURES: Mutex<Option<HashMap<u64, f32>>> = Mutex::new(None);
/// Each pen pointer's last barrel roll, radians clockwise, for pens that give one.
static ROLLS: Mutex<Option<HashMap<u64, f32>>> = Mutex::new(None);

/// The pen pointer last heard from, while it is near the window, for the brush cursor to turn as
/// it leans.
static LATEST: Mutex<Option<u64>> = Mutex::new(None);

/// Each pen pointer's last flags (its barrel button held, its eraser end down).
static FLAGS: Mutex<Option<HashMap<u64, u32>>> = Mutex::new(None);

/// How many times a pen's first and second barrel buttons went down since the app last asked.
static PRESSES: AtomicUsize = AtomicUsize::new(0);
static SECOND_PRESSES: AtomicUsize = AtomicUsize::new(0);

/// Each pointer the system has said is a pen (true) or a finger (false).
static KINDS: Mutex<Option<HashMap<u64, bool>>> = Mutex::new(None);

/// Each pen pointer the system said is built into a screen (true), so the hand is on what it
/// paints, or isn't (false).
static ON_SCREEN: Mutex<Option<HashMap<u64, bool>>> = Mutex::new(None);

/// Pointers the system took back whose lift hasn't come yet.
static TAKEN_BACK: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// A tablet's pen moving the mouse pointer (X11 and a Mac give it as the mouse): its pressure
/// (0..1) and whether its eraser end is the one on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MousePen {
    pub pressure: f32,
    pub eraser: bool,
}

/// The pen moving the mouse pointer now, while a tablet's pen is what moves it.
static MOUSE_PEN: Mutex<Option<MousePen>> = Mutex::new(None);

/// The pen's latest readings under X11, each with where it put the mouse pointer (window pixels),
/// newest last: the mouse's moves come apart from them (winit's own, on another connection), and
/// each takes the reading made where it is, not the latest, which may be ahead of it or behind.
static MOUSE_PEN_TRAIL: Mutex<VecDeque<((f32, f32), MousePen)>> = Mutex::new(VecDeque::new());
/// How many readings the trail keeps: more than come in while a slow frame is drawn.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
const TRAIL: usize = 64;

/// The mouse pointer's id among the touches (`input.rs`): a pen moving it keeps its lean there.
const MOUSE: u64 = u64::MAX;

#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
const FLAG_BARREL: u32 = 0x1;
const FLAG_INVERTED: u32 = 0x2;
const FLAG_ERASER: u32 = 0x4;
/// The pen's second barrel button held: a flag of our own (Windows's pen flags have one barrel
/// button).
const FLAG_SECOND_BARREL: u32 = 0x100;

/// The lean of the pen that is pointer `id` (a touch's id), if it is a pen that gives one.
pub fn lean(id: u64) -> Option<(f32, f32)> {
    LEANS.lock().ok()?.as_ref()?.get(&id).copied()
}

/// The pressure (0..1) of the pen that is pointer `id`, if it gives one: a touch's own force
/// leaves out no pressure at all (as a pen has as it lands and as it lifts).
pub fn pressure(id: u64) -> Option<f32> {
    PRESSURES.lock().ok()?.as_ref()?.get(&id).copied()
}

/// Pointer `id`'s pen pressure as it now is (none: it gives none).
#[cfg_attr(not(windows), allow(dead_code))]
fn pen_pressure(id: u64, pressure: Option<f32>) {
    let Ok(mut all) = PRESSURES.lock() else { return };
    let all = all.get_or_insert_with(Default::default);
    match pressure {
        Some(p) => all.insert(id, p.clamp(0.0, 1.0)),
        None => all.remove(&id),
    };
}

/// The barrel roll of the pen that is pointer `id`, if it is a pen that gives one.
pub fn roll(id: u64) -> Option<f32> {
    ROLLS.lock().ok()?.as_ref()?.get(&id).copied()
}

/// The direction the pen near the window leans (radians clockwise from screen-up), if a pen is
/// near.
pub fn azimuth() -> Option<f32> {
    let id = (*LATEST.lock().ok()?)?;
    Some(lean(id).map_or(0.0, |(_, towards)| towards))
}

/// Pointer `id`'s lean and roll as they now are; it is the pen last heard from.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn keep_pose(id: u64, lean: (f32, f32), roll: Option<f32>) {
    if let Ok(mut leans) = LEANS.lock() {
        leans.get_or_insert_with(Default::default).insert(id, lean);
    }
    if let Ok(mut rolls) = ROLLS.lock() {
        let rolls = rolls.get_or_insert_with(Default::default);
        match roll {
            Some(r) => rolls.insert(id, r),
            None => rolls.remove(&id),
        };
    }
    if let Ok(mut latest) = LATEST.lock() {
        *latest = Some(id);
    }
}

/// No pen is near the window any more.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pen_gone() {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = None;
    }
}

/// Whether the pen that is pointer `id` is turned over, its eraser end on the screen.
pub fn eraser_end(id: u64) -> bool {
    let flags = FLAGS.lock().ok().and_then(|f| f.as_ref()?.get(&id).copied()).unwrap_or(0);
    flags & (FLAG_INVERTED | FLAG_ERASER) != 0
}

/// Whether pointer `id` is a pen (true) or a finger (false), if the system said which: a touch
/// screen may give its fingers a pressure too, so a pressure alone doesn't make a pen.
pub fn is_pen(id: u64) -> Option<bool> {
    KINDS.lock().ok()?.as_ref()?.get(&id).copied()
}

/// The system said pointer `id` is a pen (`pen`) or a finger.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) fn pointer_kind(id: u64, pen: bool) {
    if let Ok(mut kinds) = KINDS.lock() {
        kinds.get_or_insert_with(Default::default).insert(id, pen);
    }
}

/// The system took pointer `id` back (a palm it has made out, say).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn taken_back(id: u64) {
    if let Ok(mut all) = TAKEN_BACK.lock()
        && !all.contains(&id)
    {
        all.push(id);
    }
}

/// Whether pointer `id`, lifting, is one the system took back: winit gives that as any other lift.
pub fn was_taken_back(id: u64) -> bool {
    let Ok(mut all) = TAKEN_BACK.lock() else { return false };
    let found = all.iter().position(|p| *p == id);
    found.map(|at| all.swap_remove(at)).is_some()
}

/// Whether pointer `id` is a pen that isn't on the screen it paints on: a tablet without one, or
/// a pen the system can't say of (only Windows says, so a pen display elsewhere counts too: a
/// brush outline round its tip costs less than no sign of the pen on a tablet).
pub fn away_from_screen(id: u64) -> bool {
    let on_screen = ON_SCREEN.lock().ok().and_then(|s| s.as_ref()?.get(&id).copied());
    is_pen(id) == Some(true) && on_screen != Some(true)
}

/// The system said pen `id` is built into a screen, or isn't.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn pen_on_screen(id: u64, on_screen: bool) {
    if let Ok(mut all) = ON_SCREEN.lock() {
        all.get_or_insert_with(Default::default).insert(id, on_screen);
    }
}

/// Whether the first and second barrel buttons of the pen near the window are held now.
pub fn buttons_held() -> [bool; 2] {
    let Some(id) = LATEST.lock().ok().and_then(|l| *l) else { return [false; 2] };
    let flags = FLAGS.lock().ok().and_then(|f| f.as_ref()?.get(&id).copied()).unwrap_or(0);
    [flags & FLAG_BARREL != 0, flags & FLAG_SECOND_BARREL != 0]
}

/// How many times a pen's first and second barrel buttons were pressed since the last call.
pub fn take_button_presses() -> [usize; 2] {
    [PRESSES.swap(0, Ordering::Relaxed), SECOND_PRESSES.swap(0, Ordering::Relaxed)]
}

/// The tablet pen moving the mouse pointer, if one is (none: a mouse, or a system that gives the
/// pen as a pen).
pub fn mouse_pen() -> Option<MousePen> {
    MOUSE_PEN.lock().ok().and_then(|p| *p)
}

/// The tablet pen moving the mouse pointer, as it was read with the pointer at `at` (window
/// pixels) if it was lately, else as last read; none while it isn't a pen that moves it.
pub fn mouse_pen_at(at: (f32, f32)) -> Option<MousePen> {
    let latest = mouse_pen()?;
    let near = |p: (f32, f32)| (p.0 - at.0).abs() < 1e-3 && (p.1 - at.1).abs() < 1e-3;
    let there = MOUSE_PEN_TRAIL.lock().ok().and_then(|t| t.iter().rev().find(|(p, _)| near(*p)).map(|(_, pen)| *pen));
    Some(there.unwrap_or(latest))
}

/// The pen moving the mouse pointer was read with the pointer at `at`, window pixels.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn keep_mouse_pen_at(at: (f32, f32), pen: MousePen) {
    if let Ok(mut trail) = MOUSE_PEN_TRAIL.lock() {
        if trail.len() >= TRAIL {
            trail.pop_front();
        }
        trail.push_back((at, pen));
    }
}

/// What moves the mouse pointer now: a tablet's pen with its lean, or (none) something else.
#[cfg_attr(not(unix), allow(dead_code))]
fn set_mouse_pen(pen: Option<(MousePen, (f32, f32))>) {
    if let Ok(mut p) = MOUSE_PEN.lock() {
        *p = pen.map(|(p, _)| p);
    }
    // another pointer moving it: the pen's readings are past
    if pen.is_none()
        && let Ok(mut trail) = MOUSE_PEN_TRAIL.lock()
    {
        trail.clear();
    }
    if let Ok(mut leans) = LEANS.lock() {
        let leans = leans.get_or_insert_with(Default::default);
        match pen {
            Some((_, lean)) => leans.insert(MOUSE, lean),
            None => leans.remove(&MOUSE),
        };
    }
}

/// Where a tablet pen's pressure and tilts are among its device's axes (XInput valuators): each
/// one's number and range; and whether the device is the pen's eraser end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
struct PenAxes {
    pressure: Option<(u16, f64, f64)>,
    tilt_x: Option<(u16, f64, f64)>,
    tilt_y: Option<(u16, f64, f64)>,
    eraser: bool,
}

/// A pen's reading as it last was: pressure (0..1) and tilts towards the right and the user,
/// degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
struct PenReading {
    pressure: f32,
    tilt_x: f32,
    tilt_y: f32,
}

/// The axes an XInput event carries, by number: `mask` says which (a bit each), `values` gives
/// them in that order, as fixed point (whole part, fraction of 2^32).
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn axis_values(mask: &[u32], values: &[(i32, u32)]) -> Vec<(u16, f64)> {
    let mut next = values.iter();
    let mut out = Vec::new();
    for (word, bits) in mask.iter().enumerate() {
        for bit in 0..32 {
            if bits & (1 << bit) == 0 {
                continue;
            }
            let Some(&(whole, part)) = next.next() else { return out };
            out.push(((word * 32 + bit) as u16, f64::from(whole) + f64::from(part) / 4_294_967_296.0));
        }
    }
    out
}

/// `last` with what an event's `values` change of the pen's `axes` (an axis it leaves out is as
/// it was): pressure from its range to 0..1; a tilt as degrees, its range taken as degrees when it
/// lies within ±90 (as tablets' drivers give it), else spread over ±60.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn pen_reading(axes: &PenAxes, values: &[(u16, f64)], last: PenReading) -> PenReading {
    let of = |axis: Option<(u16, f64, f64)>| {
        let (n, min, max) = axis?;
        values.iter().find(|(k, _)| *k == n).map(|&(_, v)| (v, min, max))
    };
    let tilt = |axis, was: f32| {
        of(axis).map_or(was, |(v, min, max)| {
            if min >= -90.0 && max <= 90.0 {
                v as f32
            } else {
                let mid = (min + max) / 2.0;
                ((v - mid) / ((max - min) / 2.0).max(1e-9) * 60.0) as f32
            }
        })
    };
    PenReading {
        pressure: of(axes.pressure)
            .map_or(last.pressure, |(v, min, max)| ((v - min) / (max - min).max(1e-9)).clamp(0.0, 1.0) as f32),
        tilt_x: tilt(axes.tilt_x, last.tilt_x),
        tilt_y: tilt(axes.tilt_y, last.tilt_y),
    }
}

/// Pointer `id`'s pen flags as they now are: a press of either barrel button counted.
#[cfg_attr(not(any(windows, target_os = "macos", target_os = "linux")), allow(dead_code))]
fn pen_flags(id: u64, flags: u32) {
    let Ok(mut all) = FLAGS.lock() else { return };
    let before = all.get_or_insert_with(Default::default).insert(id, flags).unwrap_or(0);
    for (flag, presses) in [(FLAG_BARREL, &PRESSES), (FLAG_SECOND_BARREL, &SECOND_PRESSES)] {
        if flags & flag != 0 && before & flag == 0 {
            presses.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The lean of a pen tilted `tilt_x` degrees towards the right and `tilt_y` towards the user
/// (each −90..90): its angle from upright and the direction its top leans, clockwise from up.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn lean_of(tilt_x: f32, tilt_y: f32) -> (f32, f32) {
    // short of flat, where the tangent runs off
    let tangent = |degrees: f32| degrees.clamp(-89.9, 89.9).to_radians().tan();
    let (x, y) = (tangent(tilt_x), tangent(tilt_y));
    (x.hypot(y).atan(), x.atan2(-y).rem_euclid(std::f32::consts::TAU))
}

/// Starts reading the pen's lean from the pointer messages to `window`.
#[cfg(windows)]
pub fn watch(window: &winit::window::Window, _wake: winit::event_loop::EventLoopProxy<()>) {
    system::watch(window);
}

/// Starts reading a tablet's pen: under X11 its pressure, lean and eraser end from its XInput
/// events; on Wayland the whole pen from the tablet protocol, what it does coming in through
/// [`take_events`], `wake` waking the app for it.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn watch(window: &winit::window::Window, wake: winit::event_loop::EventLoopProxy<()>) {
    tablet::watch(window);
    #[cfg(target_os = "linux")]
    wayland::watch(window, wake);
    #[cfg(not(target_os = "linux"))]
    drop(wake);
}

/// Starts reading a tablet pen's pressure, lean and eraser end from the system's tablet events, on
/// a Mac.
#[cfg(target_os = "macos")]
pub fn watch(_window: &winit::window::Window, _wake: winit::event_loop::EventLoopProxy<()>) {
    tablet_events::watch();
}

#[cfg(not(any(windows, unix)))]
pub fn watch(_window: &winit::window::Window, _wake: winit::event_loop::EventLoopProxy<()>) {}

/// Stops reading the pen, before the window's connection closes.
pub fn stop() {
    #[cfg(target_os = "linux")]
    wayland::stop();
}

/// The cursor the interface wants, shown for a pen hovering the window as for the mouse.
pub fn set_cursor(_icon: egui::CursorIcon) {
    #[cfg(target_os = "linux")]
    wayland::set_cursor(_icon);
}

/// What a pen read by the app itself did: where in the window it is in logical pixels, its
/// pressure 0..1.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum PenEvent {
    /// Over the window, its tip up.
    Hover {
        id: u64,
        x: f64,
        y: f64,
    },
    Down {
        id: u64,
        x: f64,
        y: f64,
        pressure: f32,
    },
    Move {
        id: u64,
        x: f64,
        y: f64,
        pressure: f32,
    },
    Up {
        id: u64,
        x: f64,
        y: f64,
        pressure: f32,
    },
    /// Away from the window.
    Gone,
}

/// What pens read by the app itself did since the last call, as the window events winit makes of
/// a pen on Windows (touches with a force, and the pointer moving as it hovers), for a window at
/// `scale` physical pixels a logical one.
pub fn take_events(scale: f64) -> Vec<winit::event::WindowEvent> {
    #[cfg(target_os = "linux")]
    let events = wayland::take_events();
    #[cfg(not(target_os = "linux"))]
    let events: Vec<PenEvent> = Vec::new();
    events.into_iter().map(|e| window_event(e, scale)).collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn window_event(event: PenEvent, scale: f64) -> winit::event::WindowEvent {
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, Force, Touch, TouchPhase, WindowEvent};
    let device_id = DeviceId::dummy();
    let touch = |id, x: f64, y: f64, phase, pressure: f32| {
        WindowEvent::Touch(Touch {
            device_id,
            phase,
            location: PhysicalPosition::new(x * scale, y * scale),
            force: Some(Force::Normalized(f64::from(pressure))),
            id,
        })
    };
    match event {
        PenEvent::Hover { x, y, .. } => {
            WindowEvent::CursorMoved { device_id, position: PhysicalPosition::new(x * scale, y * scale) }
        }
        PenEvent::Down { id, x, y, pressure } => touch(id, x, y, TouchPhase::Started, pressure),
        PenEvent::Move { id, x, y, pressure } => touch(id, x, y, TouchPhase::Moved, pressure),
        PenEvent::Up { id, x, y, pressure } => touch(id, x, y, TouchPhase::Ended, pressure),
        PenEvent::Gone => WindowEvent::CursorLeft { device_id },
    }
}

#[cfg(target_os = "linux")]
mod wayland;

/// A Mac tablet's tilt (each −1..1, towards the right and up the tablet) as degrees towards the
/// right and towards the user, spread over ±60 as the X11 tilts in a raw range are.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn tablet_tilt((x, y): (f64, f64)) -> (f32, f32) {
    ((x.clamp(-1.0, 1.0) * 60.0) as f32, (-y.clamp(-1.0, 1.0) * 60.0) as f32)
}

/// A Mac tablet's pen, which winit gives as the mouse: each mouse event a pen makes carries its
/// tablet point (pressure, tilt, buttons), read by a monitor of the app's events that sees it
/// before winit does; which end is near the tablet comes with its proximity. Unsafe as the
/// system's calls are.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod tablet_events {
    use std::cell::Cell;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventButtonMask, NSEventMask, NSEventSubtype, NSEventType, NSPointingDeviceType};

    use super::{FLAG_BARREL, MOUSE, MousePen};

    pub fn watch() {
        let mask = NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::OtherMouseDragged
            | NSEventMask::MouseMoved
            | NSEventMask::TabletPoint
            | NSEventMask::TabletProximity;
        // the pen's end near the tablet is its eraser (as its proximity said)
        let eraser = Cell::new(false);
        let monitor = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: the system hands the monitor a live event, for as long as it runs
            read(unsafe { event.as_ref() }, &eraser);
            event.as_ptr()
        });
        // SAFETY: the monitor gives back the event it was given, for winit to take as ever
        match unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &monitor) } {
            // it watches for as long as the app runs
            Some(watching) => std::mem::forget(watching),
            None => log::warn!("a tablet's pen isn't read: no monitor of the app's events"),
        }
    }

    /// A mouse or tablet event: the pen it comes from kept as what moves the mouse pointer, or
    /// none (the mouse's own event, or the pen gone from the tablet).
    fn read(event: &NSEvent, eraser: &Cell<bool>) {
        let kind = event.r#type();
        // a subtype is only asked of a mouse's event (a tablet's own has none)
        let subtype =
            (kind != NSEventType::TabletPoint && kind != NSEventType::TabletProximity).then(|| event.subtype());
        if kind == NSEventType::TabletProximity || subtype == Some(NSEventSubtype::TabletProximity) {
            let near = event.isEnteringProximity();
            eraser.set(near && event.pointingDeviceType() == NSPointingDeviceType::Eraser);
            if !near {
                super::set_mouse_pen(None);
            }
        } else if kind == NSEventType::TabletPoint || subtype == Some(NSEventSubtype::TabletPoint) {
            let tilt = event.tilt();
            let (x, y) = super::tablet_tilt((tilt.x, tilt.y));
            let pen = MousePen { pressure: event.pressure().clamp(0.0, 1.0), eraser: eraser.get() };
            super::set_mouse_pen(Some((pen, super::lean_of(x, y))));
            // a side button held is the barrel's
            let sides = NSEventButtonMask::PenLowerSide | NSEventButtonMask::PenUpperSide;
            super::pen_flags(MOUSE, if event.buttonMask().intersects(sides) { FLAG_BARREL } else { 0 });
        } else {
            // the mouse or the trackpad moves the pointer
            super::set_mouse_pen(None);
        }
    }
}

/// A tablet's pen under X11: its devices found by their pressure axis, their XInput 2 events read
/// on a connection of our own, on a thread of its own.
#[cfg(all(unix, not(target_os = "macos")))]
mod tablet {
    use std::collections::HashMap;
    use std::error::Error;

    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xinput::{self, ConnectionExt as _, DeviceClassData, XIEventMask};
    use x11rb::protocol::xproto::ConnectionExt as _;

    use super::{MousePen, PenAxes, PenReading};

    pub fn watch(window: &winit::window::Window) {
        let Ok(handle) = window.window_handle() else { return };
        let id = match handle.as_raw() {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            // Wayland: the tablet protocol reads it there (`wayland`)
            _ => return,
        };
        let started = std::thread::Builder::new().name("tablet pen".into()).spawn(move || {
            if let Err(e) = read(id) {
                log::warn!("a tablet's pen isn't read: {e}");
            }
        });
        if let Err(e) = started {
            log::warn!("a tablet's pen isn't read: {e}");
        }
    }

    /// The pens' events on `window`, each kept as what moves the mouse pointer; a pointer that
    /// isn't a pen moving it lets that go. Devices plugged in or out are found again.
    fn read(window: u32) -> Result<(), Box<dyn Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        conn.xinput_xi_query_version(2, 2)?.reply()?;
        let root = conn.setup().roots.get(screen).map(|s| s.root).ok_or("no screen")?;
        let select = |on: u32, mask: XIEventMask| {
            conn.xinput_xi_select_events(
                on,
                &[xinput::EventMask { deviceid: xinput::Device::ALL.into(), mask: vec![mask] }],
            )
        };
        select(window, XIEventMask::MOTION | XIEventMask::BUTTON_PRESS | XIEventMask::BUTTON_RELEASE)?;
        select(root, XIEventMask::HIERARCHY)?;
        conn.flush()?;
        let mut pens = find_pens(&conn)?;
        let mut last: HashMap<u16, PenReading> = HashMap::new();
        loop {
            match conn.wait_for_event()? {
                Event::XinputHierarchy(_) => pens = find_pens(&conn)?,
                Event::XinputMotion(e) | Event::XinputButtonPress(e) | Event::XinputButtonRelease(e)
                    if e.deviceid == e.sourceid =>
                {
                    let Some(axes) = pens.get(&e.deviceid) else {
                        super::set_mouse_pen(None);
                        continue;
                    };
                    let values: Vec<(i32, u32)> = e.axisvalues.iter().map(|v| (v.integral, v.frac)).collect();
                    let was = last.get(&e.deviceid).copied().unwrap_or_default();
                    let now = super::pen_reading(axes, &super::axis_values(&e.valuator_mask, &values), was);
                    last.insert(e.deviceid, now);
                    let pen = MousePen { pressure: now.pressure, eraser: axes.eraser };
                    super::set_mouse_pen(Some((pen, super::lean_of(now.tilt_x, now.tilt_y))));
                    // where it put the pointer, as winit has it (fixed point, 16 bits of fraction)
                    let at = |v: xinput::Fp1616| (f64::from(v) / 65536.0) as f32;
                    super::keep_mouse_pen_at((at(e.event_x), at(e.event_y)), pen);
                }
                _ => {}
            }
        }
    }

    /// The pointer devices that are pens: those with a pressure axis, by id.
    fn find_pens(conn: &impl Connection) -> Result<HashMap<u16, PenAxes>, Box<dyn Error>> {
        let reply = conn.xinput_xi_query_device(xinput::Device::ALL)?.reply()?;
        let mut names: HashMap<u32, String> = HashMap::new();
        let mut out = HashMap::new();
        for info in reply.infos {
            if info.type_ != xinput::DeviceType::SLAVE_POINTER {
                continue;
            }
            let name = String::from_utf8_lossy(&info.name).to_lowercase();
            let mut axes = PenAxes { eraser: name.contains("eraser"), ..PenAxes::default() };
            for class in &info.classes {
                let DeviceClassData::Valuator(v) = &class.data else { continue };
                if v.label == 0 {
                    continue;
                }
                if let std::collections::hash_map::Entry::Vacant(e) = names.entry(v.label) {
                    e.insert(String::from_utf8_lossy(&conn.get_atom_name(v.label)?.reply()?.name).into_owned());
                }
                let fixed = |f: xinput::Fp3232| f64::from(f.integral) + f64::from(f.frac) / 4_294_967_296.0;
                let axis = Some((v.number, fixed(v.min), fixed(v.max)));
                match names.get(&v.label).map(String::as_str) {
                    Some("Abs Pressure") => axes.pressure = axis,
                    Some("Abs Tilt X") => axes.tilt_x = axis,
                    Some("Abs Tilt Y") => axes.tilt_y = axis,
                    _ => {}
                }
            }
            if axes.pressure.is_some() {
                out.insert(info.deviceid, axes);
            }
        }
        Ok(out)
    }
}

/// The window's own procedure, put in front of winit's: unsafe as the system's calls are.
#[cfg(windows)]
#[allow(unsafe_code)]
mod system {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Controls::{POINTER_DEVICE_INFO, POINTER_DEVICE_TYPE, POINTER_DEVICE_TYPE_INTEGRATED_PEN};
    use windows::Win32::UI::Input::Pointer::{
        GetPointerDevice, GetPointerFrameInfo, GetPointerInfo, GetPointerPenInfo, GetPointerType, POINTER_INFO,
        POINTER_PEN_INFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, POINTER_INPUT_TYPE, PT_PEN, PT_TOUCH, SetWindowLongPtrW,
        WM_ENDSESSION, WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE, WNDPROC,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// What the pen info gives (`penMask`): its pressure, out of 1024, its turn and its tilts.
    const PEN_MASK_PRESSURE: u32 = 0x1;
    const PEN_MASK_ROTATION: u32 = 0x2;
    const PEN_MASK_TILT_X: u32 = 0x4;
    const PEN_MASK_TILT_Y: u32 = 0x8;
    /// A pointer message's flags (its `wparam`'s high word): the pointer touches the screen.
    const POINTER_FLAG_INCONTACT: usize = 0x4;
    /// A pointer's flags, as a message's and as a frame gives them: the system took it back, and
    /// what it did is to be undone.
    const POINTER_FLAG_CANCELED: u32 = 0x8000;

    /// winit's procedure for the window, called after ours.
    static WINIT: AtomicIsize = AtomicIsize::new(0);

    /// The pen devices named in the log so far.
    static LOGGED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

    pub fn watch(window: &winit::window::Window) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
        let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        let ours = pointer_messages as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        // SAFETY: the window is this thread's own and lives as long as the app; winit's procedure,
        // kept, is called for every message after ours
        let winit = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as usize as isize) };
        WINIT.store(winit, Ordering::Relaxed);
    }

    /// Notes what the other pointers in `id`'s frame are, and any taken back. winit takes a whole
    /// frame from its first message and skips the rest, so a second finger's own messages never
    /// come here: unnoted, a finger with a pressure would be taken for a pen, and draw whatever the
    /// finger settings say.
    fn frame_kinds(id: u32) {
        let mut count = 0;
        // SAFETY: a count to fill, no pointers asked for yet
        if unsafe { GetPointerFrameInfo(id, &mut count, None) }.is_err() || count < 2 {
            return;
        }
        let mut pointers = vec![POINTER_INFO::default(); count as usize];
        // SAFETY: room for the `count` pointers it said there are
        if unsafe { GetPointerFrameInfo(id, &mut count, Some(pointers.as_mut_ptr())) }.is_err() {
            return;
        }
        for pointer in pointers.iter().take(count as usize) {
            if pointer.pointerType == PT_PEN || pointer.pointerType == PT_TOUCH {
                super::pointer_kind(u64::from(pointer.pointerId), pointer.pointerType == PT_PEN);
            }
            if pointer.pointerFlags.0 & POINTER_FLAG_CANCELED != 0 {
                super::taken_back(u64::from(pointer.pointerId));
            }
        }
    }

    /// The kind of device pen `id` belongs to, as the system has it: built into a screen or not
    /// (from the digitizer's own report), with its name in the log the first time it is heard
    /// from.
    fn pen_device(id: u32) -> Option<POINTER_DEVICE_TYPE> {
        let mut pointer = POINTER_INFO::default();
        // SAFETY: `pointer` is an info to fill for this pointer
        unsafe { GetPointerInfo(id, &mut pointer) }.ok()?;
        let mut device = POINTER_DEVICE_INFO::default();
        // SAFETY: `device` is an info to fill for the device the system just named
        unsafe { GetPointerDevice(pointer.sourceDevice, &mut device) }.ok()?;
        let name = device.productString;
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        let name = String::from_utf16_lossy(name.get(..end).unwrap_or_default());
        if LOGGED.lock().is_ok_and(|mut logged| logged.insert(name.clone())) {
            log::info!("pen: {name} (device type {})", device.pointerDeviceType.0);
        }
        Some(device.pointerDeviceType)
    }

    /// A pen's pointer message: its pressure and tilt kept by pointer, then the message on to
    /// winit (a hovering pen's to the system, as the mouse's). And the session's end noted.
    unsafe extern "system" fn pointer_messages(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // Windows shutting down or signing out ends the app without the event loop's end: the run
        // ended all the same, so the next start keeps its graphics backend (else it would move on,
        // as after a crash; the artwork open is autosaved)
        if msg == WM_ENDSESSION && wparam.0 != 0 {
            crate::startup::gpu_ended();
        }
        let mut hovering = false;
        if msg == WM_POINTERDOWN || msg == WM_POINTERUPDATE || msg == WM_POINTERUP {
            // the pointer's id is the low word of `wparam`
            let id = (wparam.0 & 0xFFFF) as u32;
            // a pen or a finger, as the system knows it (a touch screen's fingers may have a pressure)
            let mut kind = POINTER_INPUT_TYPE::default();
            // SAFETY: `kind` is a type to fill for this pointer
            if unsafe { GetPointerType(id, &mut kind) }.is_ok() && (kind == PT_PEN || kind == PT_TOUCH) {
                super::pointer_kind(u64::from(id), kind == PT_PEN);
            }
            frame_kinds(id);
            if (wparam.0 >> 16) as u32 & POINTER_FLAG_CANCELED != 0 {
                super::taken_back(u64::from(id));
            }
            if msg == WM_POINTERDOWN && kind == PT_PEN {
                super::pen_on_screen(u64::from(id), pen_device(id) == Some(POINTER_DEVICE_TYPE_INTEGRATED_PEN));
            }
            // a pen held above the screen (not touching it)
            let touching = (wparam.0 >> 16) & POINTER_FLAG_INCONTACT != 0;
            hovering = msg == WM_POINTERUPDATE && kind == PT_PEN && !touching;
            let mut info = POINTER_PEN_INFO::default();
            // SAFETY: `info` is a pen info to fill; a pointer that isn't a pen fails and is left
            if unsafe { GetPointerPenInfo(id, &mut info) }.is_ok() {
                let tilted = info.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y) != 0;
                let lean = if tilted { super::lean_of(info.tiltX as f32, info.tiltY as f32) } else { (0.0, 0.0) };
                // degrees clockwise
                let roll = (info.penMask & PEN_MASK_ROTATION != 0).then(|| (info.rotation as f32).to_radians());
                super::keep_pose(u64::from(id), lean, roll);
                super::pen_flags(u64::from(id), info.penFlags);
                let pressed = info.penMask & PEN_MASK_PRESSURE != 0;
                super::pen_pressure(u64::from(id), pressed.then(|| info.pressure as f32 / 1024.0));
            }
        }
        // a hovering pen's moves to the system's own handling, which makes them the mouse's, so the
        // brush cursor follows it (winit would take them as touches moving that never began, and
        // the cursor would stay where the mouse last was)
        if hovering {
            // SAFETY: the message as it came, to the system's default procedure
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        // SAFETY: the procedure winit set, as SetWindowLongPtrW gave it back
        let winit: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(WINIT.load(Ordering::Relaxed)) };
        unsafe { CallWindowProcW(winit, hwnd, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_barrel_button_counts_once_each_time_it_goes_down() {
        take_button_presses();
        pen_flags(901, 0);
        pen_flags(901, FLAG_BARREL);
        pen_flags(901, FLAG_BARREL);
        pen_flags(901, 0);
        pen_flags(901, FLAG_BARREL);
        assert_eq!(take_button_presses(), [2, 0]);
        // the second button counts apart, held with the first or alone
        pen_flags(903, FLAG_SECOND_BARREL);
        pen_flags(903, FLAG_BARREL | FLAG_SECOND_BARREL);
        pen_flags(903, 0);
        pen_flags(903, FLAG_SECOND_BARREL);
        assert_eq!(take_button_presses(), [1, 2]);
        assert!(!eraser_end(901));
        pen_flags(902, FLAG_INVERTED);
        assert!(eraser_end(902));
    }

    #[test]
    fn a_pens_pressure_is_kept_as_it_last_was_none_being_kept_too() {
        assert_eq!(pressure(921), None);
        pen_pressure(921, Some(0.0));
        assert_eq!(pressure(921), Some(0.0));
        pen_pressure(921, Some(1.5));
        assert_eq!(pressure(921), Some(1.0));
        pen_pressure(921, None);
        assert_eq!(pressure(921), None);
    }

    #[test]
    fn the_mouse_pointer_takes_the_pens_reading_made_where_it_is() {
        let pen = |pressure| MousePen { pressure, eraser: false };
        set_mouse_pen(None);
        keep_mouse_pen_at((10.0, 20.0), pen(0.2));
        assert_eq!(mouse_pen_at((10.0, 20.0)), None, "the mouse moves it");
        set_mouse_pen(Some((pen(0.2), (0.0, 0.0))));
        keep_mouse_pen_at((10.0, 20.0), pen(0.2));
        // read on ahead of the mouse's moves: each move takes the reading where it is
        for (x, pressure) in [(11.0, 0.3), (12.5, 0.5), (14.0, 0.7)] {
            let read = pen(pressure);
            set_mouse_pen(Some((read, (0.0, 0.0))));
            keep_mouse_pen_at((x, 20.0), read);
        }
        assert_eq!(mouse_pen_at((10.0, 20.0)), Some(pen(0.2)));
        assert_eq!(mouse_pen_at((12.5, 20.0)), Some(pen(0.5)));
        // held still as it presses harder: the latest there
        keep_mouse_pen_at((10.0, 20.0), pen(0.9));
        assert_eq!(mouse_pen_at((10.0, 20.0)), Some(pen(0.9)));
        // somewhere not yet read: as last read
        assert_eq!(mouse_pen_at((30.0, 20.0)), Some(pen(0.7)));
        set_mouse_pen(None);
        assert_eq!(mouse_pen_at((12.5, 20.0)), None);
    }

    #[test]
    fn a_pointer_taken_back_is_told_once() {
        assert!(!was_taken_back(921));
        taken_back(921);
        taken_back(921);
        assert!(!was_taken_back(922));
        assert!(was_taken_back(921));
        assert!(!was_taken_back(921));
    }

    #[test]
    fn a_pointer_is_a_pen_or_a_finger_as_the_system_said() {
        assert_eq!(is_pen(911), None);
        pointer_kind(911, false);
        pointer_kind(912, true);
        assert_eq!((is_pen(911), is_pen(912)), (Some(false), Some(true)));
    }

    #[test]
    fn a_tablets_axes_give_its_pressure_and_tilts() {
        // axes 2 (pressure 0..65535), 3 and 4 (tilts, degrees) in a mask with gaps
        let values = axis_values(&[0b11100, 0b1], &[(32768, 0), (-30, 0), (15, 1 << 31), (7, 0)]);
        assert_eq!(values, [(2, 32768.0), (3, -30.0), (4, 15.5), (32, 7.0)]);
        let axes = PenAxes {
            pressure: Some((2, 0.0, 65535.0)),
            tilt_x: Some((3, -64.0, 63.0)),
            tilt_y: Some((4, -64.0, 63.0)),
            eraser: false,
        };
        let now = pen_reading(&axes, &values, PenReading::default());
        assert!((now.pressure - 0.5).abs() < 1e-3 && now.tilt_x == -30.0 && now.tilt_y == 15.5, "{now:?}");
        // an event that leaves an axis out keeps it as it was
        let later = pen_reading(&axes, &axis_values(&[0b100], &[(65535, 0)]), now);
        assert_eq!((later.pressure, later.tilt_x, later.tilt_y), (1.0, -30.0, 15.5));
        // tilts in a raw range are spread over ±60 degrees
        let raw = PenAxes { tilt_x: Some((3, 0.0, 1000.0)), ..axes };
        assert_eq!(pen_reading(&raw, &[(3, 1000.0)], now).tilt_x, 60.0);
    }

    #[test]
    fn a_mac_tablets_tilt_becomes_degrees_towards_the_right_and_the_user() {
        assert_eq!(tablet_tilt((0.0, 0.0)), (0.0, 0.0));
        // full tilt right and up the tablet (away from the user); past full is full
        assert_eq!(tablet_tilt((1.0, 1.0)), (60.0, -60.0));
        assert_eq!(tablet_tilt((-0.5, -2.0)), (-30.0, 60.0));
        // tilted towards the user, its top leans down the screen
        let (x, y) = tablet_tilt((0.0, -0.5));
        let (tilt, towards) = lean_of(x, y);
        assert!((tilt - 30f32.to_radians()).abs() < 1e-4 && (towards - std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn a_tilt_becomes_a_lean_clockwise_from_up() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let (tilt, _) = lean_of(0.0, 0.0);
        assert!(near(tilt, 0.0));
        // leaning right, towards the user, left, away
        for ((x, y), direction) in
            [((30.0, 0.0), 90.0), ((0.0, 30.0), 180.0), ((-30.0, 0.0), 270.0), ((0.0, -30.0), 0.0)]
        {
            let (tilt, towards) = lean_of(x, y);
            assert!(near(tilt, 30f32.to_radians()), "{x} {y}: {tilt}");
            assert!(near(towards, f32::to_radians(direction)), "{x} {y}: {towards}");
        }
        // flat on the screen, still leaning right
        let (tilt, towards) = lean_of(90.0, 0.0);
        assert!((tilt - std::f32::consts::FRAC_PI_2).abs() < 0.01 && near(towards, std::f32::consts::FRAC_PI_2));
    }

    #[test]
    fn a_pen_leaning_across_both_axes_takes_its_true_lean() {
        // 45 degrees from upright towards the bottom right is 35.26 along each axis
        let each = 0.5f32.sqrt().atan().to_degrees();
        let (tilt, towards) = lean_of(each, each);
        assert!((tilt.to_degrees() - 45.0).abs() < 1e-3, "{tilt}");
        assert!((towards.to_degrees() - 135.0).abs() < 1e-3, "{towards}");
    }
}
