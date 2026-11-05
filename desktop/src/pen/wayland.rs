//! A pen on Wayland, which winit doesn't read (the compositor makes a mouse of it, at full
//! pressure): the tablet protocol's tools, on winit's own connection (the window's surface is known
//! only there), read on a thread of their own and handed to the app as the touches a pen makes on
//! Windows. With the tablets bound the compositor no longer makes a mouse of the pen, so its
//! hovering, its presses and its cursor all come from here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rustix::event::{PollFd, PollFlags, Timespec};
use wayland_client::backend::{Backend, ObjectId, WaylandError};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_manager_v2::ZwpTabletManagerV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2;
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_pad_v2::{self, ZwpTabletPadV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_seat_v2::{self, ZwpTabletSeatV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_tool_v2::{self, ZwpTabletToolV2};
use wayland_protocols::wp::tablet::zv2::client::zwp_tablet_v2::ZwpTabletV2;
use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

use super::{FLAG_BARREL, FLAG_ERASER, FLAG_SECOND_BARREL, PenEvent};

/// The pens' ids, apart from the platform's touches and the mouse's.
const FIRST_ID: u64 = 0x5045_4E00_0000;
/// The barrel's buttons (linux/input-event-codes.h: BTN_STYLUS, BTN_STYLUS2, BTN_STYLUS3).
const BARREL_BUTTONS: [u32; 3] = [0x14b, 0x14c, 0x149];
/// The first barrel button (BTN_STYLUS); the others are its second (a third, where a pen has one,
/// does as the second).
const FIRST_BARREL: u32 = 0x14b;
/// How long the reader waits on the connection before looking whether to stop.
const WAIT: Timespec = Timespec { tv_sec: 0, tv_nsec: 100_000_000 };

/// What the pens did, for the app to take.
static EVENTS: Mutex<Vec<PenEvent>> = Mutex::new(Vec::new());
/// The reader's thread, and its word to stop.
static READER: Mutex<Option<(JoinHandle<()>, Arc<AtomicBool>)>> = Mutex::new(None);
/// The cursor shown for the pens over the window.
static CURSOR: Mutex<Option<Cursor>> = Mutex::new(None);

pub fn watch(window: &winit::window::Window, wake: EventLoopProxy<()>) {
    let (Ok(display), Ok(handle)) = (window.display_handle(), window.window_handle()) else { return };
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) = (display.as_raw(), handle.as_raw())
    else {
        // X11: the pen stays the mouse it is made
        return;
    };
    // SAFETY: the display is winit's, open while its event loop runs; the reader is stopped (by
    // `stop`, as the loop exits) before winit closes it
    #[allow(unsafe_code)]
    let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let (globals, queue) = match registry_queue_init::<Reader>(&connection) {
        Ok(g) => g,
        Err(e) => {
            log::warn!("no pen: the display's globals: {e}");
            return;
        }
    };
    let qh = queue.handle();
    // a compositor without tablets leaves the pen as the mouse it makes of it
    let Ok(manager) = globals.bind::<ZwpTabletManagerV2, _, _>(&qh, 1..=1, ()) else { return };
    globals.contents().with_list(|list| {
        for global in list.iter().filter(|g| g.interface == WlSeat::interface().name) {
            let seat = globals.registry().bind::<WlSeat, _, _>(global.name, 1, &qh, ());
            manager.get_tablet_seat(&seat, &qh, ());
        }
    });
    let shapes = globals.bind::<WpCursorShapeManagerV1, _, _>(&qh, 1..=1, ()).ok();
    if let Ok(mut cursor) = CURSOR.lock() {
        *cursor = Some(Cursor { connection: connection.clone(), icon: egui::CursorIcon::Default, near: Vec::new() });
    }
    let reader =
        Reader { surface: handle.surface.as_ptr() as usize, shapes, tools: HashMap::new(), next_id: FIRST_ID, wake };
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    match std::thread::Builder::new().name("pen".into()).spawn(move || read(queue, reader, &stopped)) {
        Ok(thread) => {
            if let Ok(mut r) = READER.lock() {
                *r = Some((thread, stop));
            }
        }
        Err(e) => log::warn!("no pen: its thread: {e}"),
    }
}

pub fn stop() {
    if let Some((thread, stop)) = READER.lock().ok().and_then(|mut r| r.take()) {
        stop.store(true, Ordering::Relaxed);
        let _ = thread.join();
    }
    if let Ok(mut cursor) = CURSOR.lock() {
        *cursor = None;
    }
}

pub fn take_events() -> Vec<PenEvent> {
    EVENTS.lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default()
}

pub fn set_cursor(icon: egui::CursorIcon) {
    let Ok(mut cursor) = CURSOR.lock() else { return };
    let Some(cursor) = cursor.as_mut().filter(|c| c.icon != icon) else { return };
    cursor.icon = icon;
    for near in &cursor.near {
        near.show(icon);
    }
    let _ = cursor.connection.flush();
}

/// The cursor for the pens near the window.
struct Cursor {
    connection: Connection,
    icon: egui::CursorIcon,
    near: Vec<Near>,
}

/// A pen over the window: the serial its cursor is set with.
struct Near {
    tool: ZwpTabletToolV2,
    shape: Option<WpCursorShapeDeviceV1>,
    serial: u32,
}

impl Near {
    fn show(&self, icon: egui::CursorIcon) {
        match (shape_of(icon), &self.shape) {
            (None, _) => self.tool.set_cursor(self.serial, None, 0, 0),
            (Some(shape), Some(device)) => device.set_shape(self.serial, shape),
            // without the shapes the compositor's own stays
            (Some(_), None) => {}
        }
    }
}

/// The system's cursor for egui's (none for none).
fn shape_of(icon: egui::CursorIcon) -> Option<Shape> {
    use egui::CursorIcon as I;
    Some(match icon {
        I::None => return None,
        I::ContextMenu => Shape::ContextMenu,
        I::Help => Shape::Help,
        I::PointingHand => Shape::Pointer,
        I::Progress => Shape::Progress,
        I::Wait => Shape::Wait,
        I::Cell => Shape::Cell,
        I::Crosshair => Shape::Crosshair,
        I::Text => Shape::Text,
        I::VerticalText => Shape::VerticalText,
        I::Alias => Shape::Alias,
        I::Copy => Shape::Copy,
        I::Move => Shape::Move,
        I::NoDrop => Shape::NoDrop,
        I::NotAllowed => Shape::NotAllowed,
        I::Grab => Shape::Grab,
        I::Grabbing => Shape::Grabbing,
        I::AllScroll => Shape::AllScroll,
        I::ResizeHorizontal => Shape::EwResize,
        I::ResizeNeSw => Shape::NeswResize,
        I::ResizeNwSe => Shape::NwseResize,
        I::ResizeVertical => Shape::NsResize,
        I::ResizeEast => Shape::EResize,
        I::ResizeSouthEast => Shape::SeResize,
        I::ResizeSouth => Shape::SResize,
        I::ResizeSouthWest => Shape::SwResize,
        I::ResizeWest => Shape::WResize,
        I::ResizeNorthWest => Shape::NwResize,
        I::ResizeNorth => Shape::NResize,
        I::ResizeNorthEast => Shape::NeResize,
        I::ResizeColumn => Shape::ColResize,
        I::ResizeRow => Shape::RowResize,
        I::ZoomIn => Shape::ZoomIn,
        I::ZoomOut => Shape::ZoomOut,
        _ => Shape::Default,
    })
}

/// Reads the connection for the pens' events until told to stop. The reads share the socket with
/// winit's (libwayland hands each queue its own).
fn read(mut queue: EventQueue<Reader>, mut reader: Reader, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = queue.dispatch_pending(&mut reader) {
            log::warn!("the pen's events: {e}");
            return;
        }
        let _ = queue.flush();
        let Some(guard) = queue.prepare_read() else { continue };
        let mut fds = [PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN)];
        if !matches!(rustix::event::poll(&mut fds, Some(&WAIT)), Ok(n) if n > 0) {
            // nothing yet: the read is let go, to look again
            continue;
        }
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                log::warn!("the pen's connection: {e}");
                return;
            }
        }
    }
}

/// The reader's state: the window's surface, and each tool as its frames build up.
struct Reader {
    surface: usize,
    shapes: Option<WpCursorShapeManagerV1>,
    tools: HashMap<ObjectId, Tool>,
    next_id: u64,
    wake: EventLoopProxy<()>,
}

#[derive(Default)]
struct Tool {
    id: u64,
    eraser: bool,
    /// Over the window (in proximity on its surface), and leaving it this frame.
    over: bool,
    leaving: bool,
    at: (f64, f64),
    pressure: f32,
    tilt: Option<(f64, f64)>,
    /// Its barrel roll, radians clockwise, for a pen that gives one.
    roll: Option<f32>,
    /// The tip on the tablet, and as the app was last told.
    tip: bool,
    told_tip: bool,
    /// The barrel's buttons held.
    held: Vec<u32>,
}

impl Reader {
    /// A tool's frame: what it did since the last, handed on.
    fn frame(&mut self, key: &ObjectId) {
        let Some(t) = self.tools.get_mut(key) else { return };
        if !t.over && !t.leaving {
            return;
        }
        let lean = t.tilt.map_or((0.0, 0.0), |(x, y)| super::lean_of(x as f32, y as f32));
        super::keep_pose(t.id, lean, t.roll);
        super::pointer_kind(t.id, true);
        let first = if t.held.contains(&FIRST_BARREL) { FLAG_BARREL } else { 0 };
        let second = if t.held.iter().any(|b| *b != FIRST_BARREL) { FLAG_SECOND_BARREL } else { 0 };
        let barrel = first | second;
        super::pen_flags(t.id, barrel | if t.eraser { FLAG_ERASER } else { 0 });
        let (id, (x, y), pressure) = (t.id, t.at, t.pressure);
        let mut out = Vec::new();
        match (t.tip && !t.leaving, t.told_tip) {
            (true, false) => out.push(PenEvent::Down { id, x, y, pressure }),
            (true, true) => out.push(PenEvent::Move { id, x, y, pressure }),
            (false, true) => out.push(PenEvent::Up { id, x, y, pressure }),
            (false, false) if !t.leaving => out.push(PenEvent::Hover { id, x, y }),
            (false, false) => {}
        }
        t.told_tip = t.tip && !t.leaving;
        if t.leaving {
            super::pen_gone();
            out.push(PenEvent::Gone);
            t.over = false;
            t.leaving = false;
            t.tip = false;
        }
        if let Ok(mut events) = EVENTS.lock() {
            events.extend(out);
        }
        let _ = self.wake.send_event(());
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for Reader {
    fn event(
        reader: &mut Reader,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Reader>,
    ) {
        use zwp_tablet_tool_v2::Event as E;
        let key = tool.id();
        if !reader.tools.contains_key(&key) {
            let id = reader.next_id;
            reader.next_id += 1;
            reader.tools.insert(key.clone(), Tool { id, ..Tool::default() });
        }
        if let E::Frame { .. } = event {
            reader.frame(&key);
            return;
        }
        let Some(t) = reader.tools.get_mut(&key) else { return };
        match event {
            E::Type { tool_type: WEnum::Value(kind) } => t.eraser = kind == zwp_tablet_tool_v2::Type::Eraser,
            E::ProximityIn { serial, surface, .. } => {
                t.over = surface.id().as_ptr() as usize == reader.surface;
                t.leaving = false;
                if t.over
                    && let Ok(mut cursor) = CURSOR.lock()
                    && let Some(cursor) = cursor.as_mut()
                {
                    cursor.near.retain(|n| n.tool != *tool);
                    let shape = reader.shapes.as_ref().map(|m| m.get_tablet_tool_v2(tool, qh, ()));
                    let near = Near { tool: tool.clone(), shape, serial };
                    near.show(cursor.icon);
                    cursor.near.push(near);
                }
            }
            E::ProximityOut => {
                t.leaving = t.over;
                if let Ok(mut cursor) = CURSOR.lock()
                    && let Some(cursor) = cursor.as_mut()
                {
                    cursor.near.retain(|n| {
                        let keep = n.tool != *tool;
                        if !keep && let Some(shape) = &n.shape {
                            shape.destroy();
                        }
                        keep
                    });
                }
            }
            E::Down { .. } => t.tip = true,
            E::Up => t.tip = false,
            E::Motion { x, y } => t.at = (x, y),
            E::Pressure { pressure } => t.pressure = pressure as f32 / 65535.0,
            E::Tilt { tilt_x, tilt_y } => t.tilt = Some((tilt_x, tilt_y)),
            // degrees clockwise from the pen's own neutral turn
            E::Rotation { degrees } => t.roll = Some((degrees as f32).to_radians()),
            E::Button { button, state, .. } if BARREL_BUTTONS.contains(&button) => {
                t.held.retain(|b| *b != button);
                if state == WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed) {
                    t.held.push(button);
                }
            }
            E::Removed => {
                reader.tools.remove(&key);
                tool.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for Reader {
    fn event(
        _: &mut Reader,
        _: &ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Reader>,
    ) {
        // the tablets, tools and pads it adds are bound by their own dispatches
    }

    event_created_child!(Reader, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadV2, ()> for Reader {
    fn event(
        _: &mut Reader,
        _: &ZwpTabletPadV2,
        _: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Reader>,
    ) {
    }

    event_created_child!(Reader, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for Reader {
    fn event(
        _: &mut Reader,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Reader>,
    ) {
    }

    // bound at version 1, before the dials
    event_created_child!(Reader, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

/// What the reader binds and says nothing to it it needs.
macro_rules! ignored {
    ($($proxy:ty),*) => {$(
        impl Dispatch<$proxy, ()> for Reader {
            fn event(_: &mut Reader, _: &$proxy, _: <$proxy as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Reader>) {}
        }
    )*};
}

ignored!(
    WlSeat,
    ZwpTabletManagerV2,
    ZwpTabletV2,
    ZwpTabletPadRingV2,
    ZwpTabletPadStripV2,
    WpCursorShapeManagerV1,
    WpCursorShapeDeviceV1
);

impl Dispatch<WlRegistry, GlobalListContents> for Reader {
    fn event(
        _: &mut Reader,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Reader>,
    ) {
    }
}
