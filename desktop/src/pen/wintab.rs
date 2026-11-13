//! A tablet's pen through its driver's WinTab interface (`wintab32.dll`), for Windows with the
//! driver's Windows Ink turned off: the pen then reaches the window only as the mouse, without
//! pressure, so its pressure, eraser end and lean are read from WinTab's packets instead and
//! handed on as a mouse pen's (as X11 and a Mac give theirs). While the system's own pointer
//! messages carry the pen (Windows Ink on), WinTab's packets are left alone.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::{s, w};

/// WinTab's messages, from its default base: a packet is waiting, and the pen came near or went.
pub const WT_PACKET: u32 = 0x7FF0;
pub const WT_PROXIMITY: u32 = 0x7FF5;

/// What [`WTInfoW`] is asked: the default system context, and a device's name, pressure and
/// orientation axes.
const WTI_DEFSYSCTX: u32 = 4;
const WTI_DEVICES: u32 = 100;
const DVC_NAME: u32 = 1;
const DVC_NPRESSURE: u32 = 15;
const DVC_ORIENTATION: u32 = 17;
/// A context's options: it moves the system's cursor, and sends its messages to the window.
const CXO_SYSTEM: u32 = 0x0001;
const CXO_MESSAGES: u32 = 0x0004;
/// What a packet carries, a bit each, laid out in this order: [`Packet`]'s fields.
const PK_CURSOR: u32 = 0x0020;
const PK_BUTTONS: u32 = 0x0040;
const PK_NORMAL_PRESSURE: u32 = 0x0400;
const PK_ORIENTATION: u32 = 0x1000;
const PACKET_DATA: u32 = PK_CURSOR | PK_BUTTONS | PK_NORMAL_PRESSURE | PK_ORIENTATION;

/// WinTab's context description (its `LOGCONTEXTW`, 212 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct LogContext {
    name: [u16; 40],
    options: u32,
    status: u32,
    locks: u32,
    msg_base: u32,
    device: u32,
    pkt_rate: u32,
    pkt_data: u32,
    pkt_mode: u32,
    move_mask: u32,
    btn_dn_mask: u32,
    btn_up_mask: u32,
    in_org: [i32; 3],
    in_ext: [i32; 3],
    out_org: [i32; 3],
    out_ext: [i32; 3],
    sens: [u32; 3],
    sys_mode: i32,
    sys_org: [i32; 2],
    sys_ext: [i32; 2],
    sys_sens: [u32; 2],
}

/// One of a device's axes (its `AXIS`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Axis {
    min: i32,
    max: i32,
    units: u32,
    resolution: u32,
}

/// A packet as [`PACKET_DATA`] lays it out: the cursor (which end of which pen), the buttons
/// held, the pressure, and the orientation (azimuth and altitude in tenths of a degree, twist).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Packet {
    cursor: u32,
    buttons: u32,
    pressure: u32,
    azimuth: i32,
    altitude: i32,
    twist: i32,
}

type WtInfo = unsafe extern "system" fn(u32, u32, *mut c_void) -> u32;
type WtOpen = unsafe extern "system" fn(HWND, *mut LogContext, i32) -> *mut c_void;
type WtPacket = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32;
type WtOverlap = unsafe extern "system" fn(*mut c_void, i32) -> i32;

/// The driver's calls and the context opened on the window, with the device's pressure range
/// and whether it gives an orientation.
struct Tablet {
    packet: WtPacket,
    overlap: WtOverlap,
    context: usize,
    pressure: (i32, i32),
    leans: bool,
}

static TABLET: Mutex<Option<Tablet>> = Mutex::new(None);

/// When the system's pointer messages last carried a pen (ms since [`started`]): while lately,
/// Windows Ink has it and WinTab's packets are left alone.
static INK_AT: AtomicU64 = AtomicU64::new(0);
/// How long after the system's last pen message WinTab's are taken.
const INK_QUIET: Duration = Duration::from_millis(500);

fn started() -> Instant {
    static AT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *AT.get_or_init(Instant::now)
}

/// The system's pointer messages carry a pen now (Windows Ink is on for it).
pub fn ink_pen() {
    let quiet = !ink_lately();
    INK_AT.store((started().elapsed().as_millis() as u64).max(1), Ordering::Relaxed);
    // WinTab's first packets can come before the system's first message: what they said of the
    // pen isn't the mouse's, and would stay with it (its leaving is left alone while the system
    // has the pen)
    if quiet {
        super::set_mouse_pen(None);
        super::pen_flags(super::MOUSE, 0);
    }
}

fn ink_lately() -> bool {
    let at = INK_AT.load(Ordering::Relaxed);
    at != 0 && started().elapsed().as_millis() as u64 <= at + INK_QUIET.as_millis() as u64
}

/// Opens a WinTab context on `hwnd` if a tablet's driver gives WinTab; nothing if none does.
pub fn open(hwnd: HWND) {
    // SAFETY: the driver's library, if there is one, loaded by its fixed name
    let Ok(library) = (unsafe { LoadLibraryW(w!("wintab32.dll")) }) else { return };
    let Some((info, open, packet, overlap)) = calls(library) else {
        log::warn!("WinTab: wintab32.dll lacks its calls");
        return;
    };
    let mut context = LogContext {
        name: [0; 40],
        options: 0,
        status: 0,
        locks: 0,
        msg_base: 0,
        device: 0,
        pkt_rate: 0,
        pkt_data: 0,
        pkt_mode: 0,
        move_mask: 0,
        btn_dn_mask: 0,
        btn_up_mask: 0,
        in_org: [0; 3],
        in_ext: [0; 3],
        out_org: [0; 3],
        out_ext: [0; 3],
        sens: [0; 3],
        sys_mode: 0,
        sys_org: [0; 2],
        sys_ext: [0; 2],
        sys_sens: [0; 2],
    };
    // SAFETY: a context description to fill, as large as WinTab's
    if unsafe { info(WTI_DEFSYSCTX, 0, (&raw mut context).cast()) } == 0 {
        return;
    }
    context.options |= CXO_SYSTEM | CXO_MESSAGES;
    context.pkt_data = PACKET_DATA;
    // every field as it is, not as a change from the last packet; packets whenever any changes
    context.pkt_mode = 0;
    context.move_mask = PACKET_DATA;
    let mut pressure = Axis::default();
    // SAFETY: an axis to fill
    unsafe { info(WTI_DEVICES, DVC_NPRESSURE, (&raw mut pressure).cast()) };
    let mut orientation = [Axis::default(); 3];
    // SAFETY: room for the three axes it gives
    let leans = unsafe { info(WTI_DEVICES, DVC_ORIENTATION, orientation.as_mut_ptr().cast()) } != 0
        && orientation[1].max > orientation[1].min;
    // SAFETY: the window is the app's own; the description filled above
    let opened = unsafe { open(hwnd, &mut context, 1) };
    if opened.is_null() {
        log::warn!("WinTab: the context couldn't be opened");
        return;
    }
    let mut name = [0u16; 128];
    // SAFETY: room for the name, in wide characters
    unsafe { info(WTI_DEVICES, DVC_NAME, name.as_mut_ptr().cast()) };
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = String::from_utf16_lossy(name.get(..len).unwrap_or_default());
    log::info!(
        "WinTab: {name}, pressure {}..{}, {}",
        pressure.min,
        pressure.max,
        if leans { "with lean" } else { "no lean" }
    );
    if let Ok(mut tablet) = TABLET.lock() {
        *tablet =
            Some(Tablet { packet, overlap, context: opened as usize, pressure: (pressure.min, pressure.max), leans });
    }
}

/// The driver's four calls, by name.
fn calls(library: HMODULE) -> Option<(WtInfo, WtOpen, WtPacket, WtOverlap)> {
    // SAFETY: each is looked up by the name WinTab gives it and has the signature WinTab gives it
    unsafe {
        let info = GetProcAddress(library, s!("WTInfoW"))?;
        let open = GetProcAddress(library, s!("WTOpenW"))?;
        let packet = GetProcAddress(library, s!("WTPacket"))?;
        let overlap = GetProcAddress(library, s!("WTOverlap"))?;
        Some((
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtInfo>(info),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtOpen>(open),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtPacket>(packet),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtOverlap>(overlap),
        ))
    }
}

/// The window became the active one: its context goes on top of the others, as WinTab asks.
pub fn activated() {
    let Ok(tablet) = TABLET.lock() else { return };
    let Some(t) = tablet.as_ref() else { return };
    // SAFETY: the context this app opened
    unsafe { (t.overlap)(t.context as *mut c_void, 1) };
}

/// A WinTab message to the window: a packet read and handed on as the mouse pen's, or the pen
/// gone. True if it was one.
pub fn message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    match msg {
        WT_PACKET => {
            let Ok(tablet) = TABLET.lock() else { return true };
            let Some(t) = tablet.as_ref() else { return true };
            if ink_lately() {
                return true;
            }
            let mut p = Packet::default();
            // SAFETY: the packet's serial number from the message, read into a packet laid out as
            // the context was asked to give
            if unsafe { (t.packet)(t.context as *mut c_void, wparam.0 as u32, (&raw mut p).cast()) } == 0 {
                return true;
            }
            let pen = reading(&p, t.pressure, t.leans);
            // its side switches as a pen's first and second buttons (Gesture controls' pen buttons)
            super::pen_flags(super::MOUSE, button_flags(p.buttons));
            let mut at = POINT::default();
            // SAFETY: a point to fill, then made the window's own
            let placed = unsafe { GetCursorPos(&mut at).is_ok() && ScreenToClient(hwnd, &mut at).as_bool() };
            super::set_mouse_pen(Some(pen));
            if placed {
                super::keep_mouse_pen_at((at.x as f32, at.y as f32), pen.0);
            }
            true
        }
        WT_PROXIMITY => {
            // the low word: the pen came into the tablet's reach (non-zero), or left it
            if lparam.0 & 0xFFFF == 0 && !ink_lately() {
                super::set_mouse_pen(None);
            }
            true
        }
        _ => false,
    }
}

/// A packet's buttons (a bit each: the tip, then the side switches) as a pen's flags: the lower
/// switch its first button, the upper its second.
fn button_flags(buttons: u32) -> u32 {
    let mut flags = 0;
    if buttons & 0b010 != 0 {
        flags |= super::FLAG_BARREL;
    }
    if buttons & 0b100 != 0 {
        flags |= super::FLAG_SECOND_BARREL;
    }
    flags
}

/// What a packet says: the pen's pressure (0..1 of the device's range), whether it is the eraser
/// end, and its lean (from upright, and which way, clockwise from the tablet's top).
fn reading(p: &Packet, (low, high): (i32, i32), leans: bool) -> (super::MousePen, (f32, f32)) {
    let span = (high - low).max(1) as f32;
    let pressure = ((p.pressure as i64 - low as i64) as f32 / span).clamp(0.0, 1.0);
    // a tablet's cursors come in threes (puck, pen, eraser): the third of each is an eraser end
    let eraser = p.cursor % 3 == 2;
    let lean = if leans && p.altitude > 0 {
        let tilt = ((900 - p.altitude.clamp(0, 900)) as f32 / 10.0).to_radians();
        let way = (p.azimuth.rem_euclid(3600) as f32 / 10.0).to_radians();
        (tilt, way)
    } else {
        (0.0, 0.0)
    };
    (super::MousePen { pressure, eraser }, lean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_and_axis_are_laid_out_as_wintab_has_them() {
        assert_eq!(std::mem::size_of::<LogContext>(), 212);
        assert_eq!(std::mem::size_of::<Axis>(), 16);
        assert_eq!(std::mem::size_of::<Packet>(), 24);
    }

    #[test]
    fn the_side_switches_are_the_pens_buttons() {
        assert_eq!(button_flags(0b001), 0, "the tip alone");
        assert_eq!(button_flags(0b011), super::super::FLAG_BARREL);
        assert_eq!(button_flags(0b101), super::super::FLAG_SECOND_BARREL);
        assert_eq!(button_flags(0b110), super::super::FLAG_BARREL | super::super::FLAG_SECOND_BARREL);
    }

    #[test]
    fn a_packet_reads_as_the_pens_pressure_end_and_lean() {
        let p = Packet { cursor: 1, pressure: 512, altitude: 900, azimuth: 0, ..Packet::default() };
        let (pen, lean) = reading(&p, (0, 1023), true);
        assert!((pen.pressure - 0.5).abs() < 0.01 && !pen.eraser);
        assert_eq!(lean, (0.0, 0.0), "upright");
        // the eraser end, leaning 45 degrees to the right
        let p = Packet { cursor: 2, pressure: 2047, altitude: 450, azimuth: 900, ..Packet::default() };
        let (pen, (tilt, way)) = reading(&p, (0, 2047), true);
        assert!(pen.eraser && (pen.pressure - 1.0).abs() < 1e-6);
        assert!((tilt - 45f32.to_radians()).abs() < 1e-4 && (way - 90f32.to_radians()).abs() < 1e-4);
        // a device without orientation: upright; a pressure past the range: full
        let (pen, lean) =
            reading(&Packet { cursor: 4, pressure: 5000, altitude: 300, ..Packet::default() }, (0, 1023), false);
        assert_eq!((pen.pressure, lean), (1.0, (0.0, 0.0)));
        assert!(!pen.eraser);
    }
}
