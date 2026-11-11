//! A plugin's code: a WebAssembly module, interpreted under caps on its time and memory. It gets no
//! imports at all, no files, network or layers, so all it ever sees is the numbers a hook hands it
//! and all it can do is hand some back.
//!
//! What a module exports, interface 1, in plain numbers:
//! - `procraft_position(x: f32, y: f32) -> i64`: for the hook `position`, where a sample in window
//!   pixels lands, x's bits in the low half and y's in the high.
//! - `procraft_pick_layer() -> i32`: for the hook `pick_layer`, whether the eyedropper reads the
//!   current layer (not 0) or the canvas as it is composited (0).
//! - `procraft_interface_scale(width: f32, height: f32) -> f32`: for the hook `interface_scale`, how
//!   big the interface shows (1: as measured) in a window of that size, in points at 1.
//! - `procraft_paste_mask() -> i32`: for the hook `paste_mask`, whether a picture pasted with a mask
//!   current goes into the mask (not 0). If it does, the picture goes through the module a row at a
//!   time: `procraft_row_buffer(bytes: i32) -> i32` says where in its exported `memory` a row of that
//!   many bytes can go, and for each row, written there, `procraft_paste_row(at: i32, width: i32)`
//!   turns its `width` RGBA pixels (premultiplied, top to bottom) into what the mask holds, in place.
//! - `procraft_fill_mask() -> i32`: for the hook `fill_mask`, whether a fill being let go has
//!   anything to go through (not 0). If it has, it goes through on a thread of its own, a row at a
//!   time, in bits: `procraft_fill_begin(width: i32, height: i32, seed_x: i32, seed_y: i32) -> i32`
//!   says where in its exported `memory` a row goes, `width` bits in 64-bit words, low bit first.
//!   `procraft_fill_row(y: i32)` takes each row written there, `procraft_fill_run() -> i32` works it
//!   out (not 0: something comes out), and `procraft_fill_out(y: i32)` puts each row of what comes
//!   out there. All it can do is take away: what isn't in the fill stays out of it, and the seed
//!   stays in.
//! - `procraft_fill_all_layers() -> i32`: for the hook `fill_all_layers`, whether ColorDrop fills
//!   by every layer seen (not 0) rather than by the current layer alone.
//! - `procraft_radial_segments() -> i32`: for the hook `radial_segments`, how many parts radial
//!   symmetry makes, 2 to 64.
//! - `procraft_hide_undo_notice() -> i32`: for the hook `hide_undo_notice`, whether undo and redo go
//!   by without their notice (not 0).
//! - `procraft_interface_colour(role: i32) -> i32`: for the hook `interface_colour`, the colour
//!   (0xRRGGBB) the interface draws `role` in (0: the accent, the blue of what is chosen and
//!   switched on), or -1 to leave it as it is.
//! - `procraft_pin_panels() -> i32`: for the hook `pin_panels`, whether a touch on the canvas with
//!   a panel open paints at once and leaves the panel up (not 0).
//! - `procraft_opacity_together() -> i32`: for the hook `opacity_together`, whether a layer's
//!   opacity changed while it is one of several picked sets every picked layer's (not 0).
//! - `procraft_randomise(setting: i32, seed: i32) -> f32`: for the hook `randomise_brush`, where
//!   a brush setting (by its place in [`crate::studio::RANDOMISED`]) goes, as a share of its range
//!   (0 to 1), or below 0 to leave it; `seed` is the same for every setting of one press.
//! - `procraft_colour_triangle() -> i32`: for the hook `colour_triangle`, whether the colour
//!   panel's Classic shows a triangle in place of its square (not 0).
//! - For the hook `quickshape_curve` (a curve QuickShape may snap to, the plugin's):
//!   `procraft_curve_begin(count: i32, best: i32) -> i32` gives where the stroke's `count` points go
//!   (x and y each, 32-bit floats, window pixels), `best` the kind QuickShape found first (its place
//!   among line, polyline, arc, curve, ellipse, circle, triangle, equilateral triangle,
//!   quadrilateral, rectangle, square; -1 when a curve is asked for by name); then
//!   `procraft_curve_run() -> i32` answers how many nodes the curve has (0: none; an anchor, then
//!   two controls and an anchor for each span), and `procraft_curve_nodes() -> i32` where they are.
//! - For the hook `stroke_sound` (a sound as you draw, the plugin's): `procraft_stroke_sound(frames:
//!   i32, rate: i32, speed: f32, pressure: f32, since_ms: f32, state: i32, hover_x: f32, hover_y:
//!   f32, height: f32) -> i32` gives where the next `frames` samples (32-bit floats, -1 to 1, at
//!   `rate` a second) are, for the stroke moving at `speed` (window heights a second) with
//!   `pressure`, its last input `since_ms` ago (below 0: no stroke open), `state` saying (bits) 1 a
//!   stroke is open, 2 the pen hovers (at `hover_x`, `hover_y`, window pixels, in a window `height`
//!   tall), 4 the tool erases. Called on the sound's own thread, every few milliseconds. Before the
//!   first at a rate, `procraft_sound_prepare(rate: i32) -> i32`, if it has one, as a long call.
//! - What a plugin is told if it listens: the current brush, before a sound when it changed (its
//!   name and its set's, UTF-8, into `procraft_text_buffer(len: i32) -> i32`, then
//!   `procraft_brush(name_len: i32, set_len: i32)`; its rows in the brush studio, each by its place,
//!   through `procraft_brush_control(index: i32, value: f32)`); and the file picked for a file
//!   control (into `procraft_file_buffer(index: i32, len: i32) -> i32`, then `procraft_file(index:
//!   i32, len: i32)`, `len` 0 when let go), as a long call.
//! - For the hook `inbetween` (Animation Assist's In-between draws a frame between a frame and the
//!   next, the plugin's way): `procraft_between_begin(width: i32, height: i32) -> i32` starts one
//!   of that many pixels and gives where a row of them goes (`width` × 4 bytes, premultiplied
//!   RGBA8); `procraft_between_row(frame: i32, y: i32)` says row `y` of `frame` (0 this frame, 1 the
//!   next) is there; `procraft_between_run() -> i32` draws the frame between (0: none);
//!   `procraft_between_out(y: i32)` puts its row `y` there. A long call, on fuel by the area.
//! - `procraft_control(index: i32, value: f32)`: a control's value (a switch is 0 or 1), by its
//!   place in the manifest: each as the plugin starts, then each as it changes. Needed if it has any.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use wasmi::errors::ErrorKind;
use wasmi::{
    Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TrapCode,
    TypedFunc, TypedResumableCall, ValType, WasmParams, WasmResults,
};

/// The module's name in a plugin's folder.
pub(crate) const MODULE: &str = "plugin.wasm";
/// The most a module may be, and may ask for as it runs, in bytes.
pub(crate) const MOST_MODULE: usize = 1 << 20;
const MOST_MEMORY: usize = 16 << 20;
/// What a plugin on the fill hook may ask for: a few planes of a bit a pixel, for the fills
/// [`MOST_FILL`] lets through.
const MOST_FILL_MEMORY: usize = 64 << 20;
/// What a plugin on the sound hook may ask for: its sounds' loops and the recordings picked.
const MOST_SOUND_MEMORY: usize = 128 << 20;
/// What a sound may spend (a few milliseconds of it), readying the sounds at a rate, and taking a
/// file in.
const SOUND_FUEL: u64 = 20_000_000;
const SOUND_FRAME_FUEL: u64 = 4_000;
const PREPARE_FUEL: u64 = 4_000_000_000;
const FILE_FUEL: u64 = 2_000_000_000;
/// What a plugin on the in-between hook may ask for: the two frames and the one between, and room
/// to work.
pub(crate) const MOST_BETWEEN_MEMORY: usize = 256 << 20;
/// What an in-between may spend, to start with and by the pixel.
const BETWEEN_FUEL: u64 = 20_000_000;
const BETWEEN_PIXEL_FUEL: u64 = 4096;
const MOST_TABLE: usize = 10_000;
const MOST_CALLS_DEEP: usize = 256;
/// What it may spend, in roughly one a step: to start up, and on one sample or control. A sample
/// comes every 4 ms from a 240 Hz pen, so one spent in full must stay far inside that.
const START_FUEL: u64 = 1_000_000;
const SAMPLE_FUEL: u64 = 100_000;
/// What a row may spend, by the pixel, and to start with: a picture goes through at a few times what
/// a fair conversion takes, and what never ends is stopped on its first row.
const PIXEL_FUEL: u64 = 64;
const ROW_FUEL: u64 = 10_000;
/// What a fill may spend, to start with and by the pixel of its box, and how long it may take
/// whatever it has left: a long thin maze across a third of a picture takes about 850 a pixel, and
/// fuel goes at very different speeds by what is done with it. A long call goes in slices, the time
/// looked at between them.
const FILL_FUEL: u64 = 20_000_000;
const FILL_PIXEL_FUEL: u64 = 1024;
const MOST_FILL_TIME: Duration = Duration::from_secs(30);
const FUEL_SLICE: u64 = 200_000_000;
/// The most pixels of a fill's box that go through a plugin: a whole canvas of 8192 a side.
pub(crate) const MOST_FILL: u64 = 8194 * 8194;
/// How far a sample may be moved, window pixels: further was never meant.
const MOST_MOVE: f32 = 16_384.0;
/// The scales an interface can be used at: past these nobody could reach the page to undo it.
const SCALES: std::ops::RangeInclusive<f32> = 0.5..=2.0;

const POSITION: &str = "procraft_position";
const PICK_LAYER: &str = "procraft_pick_layer";
const INTERFACE_SCALE: &str = "procraft_interface_scale";
const PASTE_MASK: &str = "procraft_paste_mask";
const ROW_BUFFER: &str = "procraft_row_buffer";
const PASTE_ROW: &str = "procraft_paste_row";
const FILL_MASK: &str = "procraft_fill_mask";
const FILL_BEGIN: &str = "procraft_fill_begin";
const FILL_ROW: &str = "procraft_fill_row";
const FILL_RUN: &str = "procraft_fill_run";
const FILL_OUT: &str = "procraft_fill_out";
const FILL_ALL_LAYERS: &str = "procraft_fill_all_layers";
const RADIAL_SEGMENTS: &str = "procraft_radial_segments";
const HIDE_UNDO_NOTICE: &str = "procraft_hide_undo_notice";
const INTERFACE_COLOUR: &str = "procraft_interface_colour";
const PIN_PANELS: &str = "procraft_pin_panels";
const OPACITY_TOGETHER: &str = "procraft_opacity_together";
const RANDOMISE: &str = "procraft_randomise";
const COLOUR_TRIANGLE: &str = "procraft_colour_triangle";
const CURVE_BEGIN: &str = "procraft_curve_begin";
const CURVE_RUN: &str = "procraft_curve_run";
const CURVE_NODES: &str = "procraft_curve_nodes";
/// What a curve may spend, to start with and by the stroke's point, and the most nodes it may have.
const CURVE_FUEL: u64 = 5_000_000;
const CURVE_POINT_FUEL: u64 = 20_000;
const MOST_CURVE_NODES: usize = 3 * 4_096 + 1;
const STROKE_SOUND: &str = "procraft_stroke_sound";
const SOUND_PREPARE: &str = "procraft_sound_prepare";
const TEXT_BUFFER: &str = "procraft_text_buffer";
const BRUSH: &str = "procraft_brush";
const BRUSH_CONTROL: &str = "procraft_brush_control";
const FILE_BUFFER: &str = "procraft_file_buffer";
const FILE: &str = "procraft_file";
const BETWEEN_BEGIN: &str = "procraft_between_begin";
const BETWEEN_ROW: &str = "procraft_between_row";
const BETWEEN_RUN: &str = "procraft_between_run";
const BETWEEN_OUT: &str = "procraft_between_out";
/// How many parts radial symmetry may be asked to make.
const RADIAL_PARTS: std::ops::RangeInclusive<i32> = 2..=64;
const MEMORY: &str = "memory";
const CONTROL: &str = "procraft_control";

/// What a plugin may step into, by the name a manifest gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hook {
    Position,
    PickLayer,
    InterfaceScale,
    PasteMask,
    FillMask,
    FillAllLayers,
    RadialSegments,
    HideUndoNotice,
    InterfaceColour,
    PinPanels,
    OpacityTogether,
    RandomiseBrush,
    ColourTriangle,
    QuickshapeCurve,
    StrokeSound,
    Inbetween,
}

impl Hook {
    pub(crate) fn named(name: &str) -> Option<Hook> {
        match name {
            "position" => Some(Hook::Position),
            "pick_layer" => Some(Hook::PickLayer),
            "interface_scale" => Some(Hook::InterfaceScale),
            "paste_mask" => Some(Hook::PasteMask),
            "fill_mask" => Some(Hook::FillMask),
            "fill_all_layers" => Some(Hook::FillAllLayers),
            "radial_segments" => Some(Hook::RadialSegments),
            "hide_undo_notice" => Some(Hook::HideUndoNotice),
            "interface_colour" => Some(Hook::InterfaceColour),
            "pin_panels" => Some(Hook::PinPanels),
            "opacity_together" => Some(Hook::OpacityTogether),
            "randomise_brush" => Some(Hook::RandomiseBrush),
            "colour_triangle" => Some(Hook::ColourTriangle),
            "quickshape_curve" => Some(Hook::QuickshapeCurve),
            "stroke_sound" => Some(Hook::StrokeSound),
            "inbetween" => Some(Hook::Inbetween),
            _ => None,
        }
    }

    /// What it lets a plugin do, for the screen that asks whether to import one.
    pub(crate) fn sentence(self) -> &'static str {
        match self {
            Hook::Position => "This plugin can change where your strokes land.",
            Hook::PickLayer => "This plugin can make the eyedropper read the current layer alone.",
            Hook::InterfaceScale => "This plugin can make the interface bigger or smaller.",
            Hook::PasteMask => "This plugin can put what you paste into the current mask.",
            Hook::FillMask => "This plugin can take parts out of a ColorDrop fill.",
            Hook::FillAllLayers => "This plugin can make ColorDrop fill by every layer you see.",
            Hook::RadialSegments => "This plugin can change how many parts radial symmetry makes.",
            Hook::HideUndoNotice => "This plugin can hide the notice undo and redo show.",
            Hook::InterfaceColour => "This plugin can change the interface's colours.",
            Hook::PinPanels => "This plugin can keep panels open while you paint.",
            Hook::OpacityTogether => "This plugin can change the opacity of all the layers you've picked at once.",
            Hook::RandomiseBrush => "This plugin can set a brush's settings at random in the brush studio.",
            Hook::ColourTriangle => "This plugin can change how the colour panel's Classic picker looks.",
            Hook::QuickshapeCurve => "This plugin can make QuickShape snap strokes to curves.",
            Hook::StrokeSound => "This plugin can play a sound as you draw.",
            Hook::Inbetween => "This plugin can draw frames between your animation's frames.",
        }
    }

    /// It as a bit, in a set of hooks.
    pub(crate) fn bit(self) -> u32 {
        1 << self as u32
    }

    /// Whether `module` exports what this hook calls.
    fn fits(self, module: &Module) -> bool {
        match self {
            Hook::Position => exports(module, POSITION, &[ValType::F32, ValType::F32], &[ValType::I64]),
            Hook::PickLayer => exports(module, PICK_LAYER, &[], &[ValType::I32]),
            Hook::FillAllLayers => exports(module, FILL_ALL_LAYERS, &[], &[ValType::I32]),
            Hook::RadialSegments => exports(module, RADIAL_SEGMENTS, &[], &[ValType::I32]),
            Hook::HideUndoNotice => exports(module, HIDE_UNDO_NOTICE, &[], &[ValType::I32]),
            Hook::InterfaceColour => exports(module, INTERFACE_COLOUR, &[ValType::I32], &[ValType::I32]),
            Hook::PinPanels => exports(module, PIN_PANELS, &[], &[ValType::I32]),
            Hook::OpacityTogether => exports(module, OPACITY_TOGETHER, &[], &[ValType::I32]),
            Hook::RandomiseBrush => exports(module, RANDOMISE, &[ValType::I32, ValType::I32], &[ValType::F32]),
            Hook::ColourTriangle => exports(module, COLOUR_TRIANGLE, &[], &[ValType::I32]),
            Hook::QuickshapeCurve => {
                exports(module, CURVE_BEGIN, &[ValType::I32, ValType::I32], &[ValType::I32])
                    && exports(module, CURVE_RUN, &[], &[ValType::I32])
                    && exports(module, CURVE_NODES, &[], &[ValType::I32])
                    && has_memory(module)
            }
            Hook::StrokeSound => {
                use ValType::{F32, I32};
                exports(module, STROKE_SOUND, &[I32, I32, F32, F32, F32, I32, F32, F32, F32], &[I32])
                    && has_memory(module)
            }
            Hook::Inbetween => {
                exports(module, BETWEEN_BEGIN, &[ValType::I32, ValType::I32], &[ValType::I32])
                    && exports(module, BETWEEN_ROW, &[ValType::I32, ValType::I32], &[])
                    && exports(module, BETWEEN_RUN, &[], &[ValType::I32])
                    && exports(module, BETWEEN_OUT, &[ValType::I32], &[])
                    && has_memory(module)
            }
            Hook::InterfaceScale => exports(module, INTERFACE_SCALE, &[ValType::F32, ValType::F32], &[ValType::F32]),
            Hook::PasteMask => {
                exports(module, PASTE_MASK, &[], &[ValType::I32])
                    && exports(module, ROW_BUFFER, &[ValType::I32], &[ValType::I32])
                    && exports(module, PASTE_ROW, &[ValType::I32, ValType::I32], &[])
                    && has_memory(module)
            }
            Hook::FillMask => {
                exports(module, FILL_MASK, &[], &[ValType::I32])
                    && exports(module, FILL_BEGIN, &[ValType::I32; 4], &[ValType::I32])
                    && exports(module, FILL_ROW, &[ValType::I32], &[])
                    && exports(module, FILL_RUN, &[], &[ValType::I32])
                    && exports(module, FILL_OUT, &[ValType::I32], &[])
                    && has_memory(module)
            }
        }
    }
}

/// Why a plugin stopped, or never ran. Each stays on its card, in plain words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    TooSlow,
    Crashed,
    Memory,
    Nonsense,
    Damaged,
}

impl Stop {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Stop::TooSlow => "Switched off: it took too long.",
            Stop::Crashed => "Switched off: it crashed.",
            Stop::Memory => "Switched off: it asked for too much memory.",
            Stop::Nonsense => "Switched off: it gave an answer that makes no sense.",
            Stop::Damaged => "Can't run: its files don't hold up.",
        }
    }
}

fn stop_of(error: &wasmi::Error) -> Stop {
    match (error.as_trap_code(), error.kind()) {
        (Some(TrapCode::OutOfFuel), _) => Stop::TooSlow,
        // with no imports, all that can keep one from starting is the caps
        (Some(TrapCode::GrowthOperationLimited), _) | (_, ErrorKind::Instantiation(_)) => Stop::Memory,
        _ => Stop::Crashed,
    }
}

/// `run`, a panic in the interpreter counting as a crash of the plugin: a file must not take the
/// app down with it.
fn guarded<T>(run: impl FnOnce() -> Result<T, Stop>) -> Result<T, Stop> {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or(Err(Stop::Crashed))
}

fn engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true).enforced_limits(EnforcedLimits::strict()).set_max_recursion_depth(MOST_CALLS_DEEP);
    Engine::new(&config)
}

/// A number as a module writes one, and how many bytes it took.
fn leb_u32(bytes: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0_u32;
    for (i, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// Whether no code section says it has no functions and then has bytes: the limits divide a
/// section's size by that count before anything is validated. Anything else is left to wasmi.
fn code_counts_hold(bytes: &[u8]) -> bool {
    const CODE: u8 = 10;
    let mut at = 8_usize;
    while let Some(&section) = bytes.get(at) {
        let Some((size, size_len)) = bytes.get(at + 1..).and_then(leb_u32) else { return true };
        let start = at + 1 + size_len;
        if section == CODE
            && let Some((0, count_len)) = bytes.get(start..).and_then(leb_u32)
            && size as usize > count_len
        {
            return false;
        }
        at = start.saturating_add(size as usize);
    }
    true
}

fn exports(module: &Module, name: &str, params: &[ValType], results: &[ValType]) -> bool {
    module.exports().any(|export| {
        export.name() == name
            && export.ty().func().is_some_and(|func| func.params() == params && func.results() == results)
    })
}

fn has_memory(module: &Module) -> bool {
    module.exports().any(|export| export.name() == MEMORY && export.ty().memory().is_some())
}

/// Whether `bytes` is a module a plugin on `hooks` can be: valid, wanting nothing from outside, and
/// exporting what it must (`controls`: it has some). Nothing in it runs.
pub(crate) fn can_run(bytes: &[u8], hooks: &[Hook], controls: bool) -> bool {
    if bytes.len() > MOST_MODULE || !code_counts_hold(bytes) {
        return false;
    }
    let fits = guarded(|| {
        let module = Module::new(&engine(), bytes).map_err(|_| Stop::Damaged)?;
        Ok(module.imports().next().is_none()
            && hooks.iter().all(|hook| hook.fits(&module))
            && (!controls || exports(&module, CONTROL, &[ValType::I32, ValType::F32], &[])))
    });
    fits.unwrap_or(false)
}

/// What the fill hook calls.
/// The sound hook's call: frames, rate, speed, pressure, since, state, hover x and y, height.
type SoundCall = TypedFunc<(i32, i32, f32, f32, f32, i32, f32, f32, f32), i32>;

/// The curve hook's calls.
struct CurveCalls {
    begin: TypedFunc<(i32, i32), i32>,
    run: TypedFunc<(), i32>,
    nodes: TypedFunc<(), i32>,
}

/// The in-between hook's calls.
struct BetweenCalls {
    begin: TypedFunc<(i32, i32), i32>,
    row: TypedFunc<(i32, i32), ()>,
    run: TypedFunc<(), i32>,
    out: TypedFunc<i32, ()>,
}

struct FillCalls {
    mask: TypedFunc<(), i32>,
    begin: TypedFunc<(i32, i32, i32, i32), i32>,
    row: TypedFunc<i32, ()>,
    run: TypedFunc<(), i32>,
    out: TypedFunc<i32, ()>,
}

/// `func` called on what is `left` of a long call's fuel, in slices, until `until`.
fn long_call<P: WasmParams, R: WasmResults>(
    store: &mut Store<StoreLimits>,
    func: &TypedFunc<P, R>,
    params: P,
    left: &mut u64,
    until: Instant,
) -> Result<R, Stop> {
    let give = (*left).min(FUEL_SLICE);
    *left -= give;
    store.set_fuel(give).map_err(|_| Stop::Damaged)?;
    let mut call = func.call_resumable(&mut *store, params).map_err(|e| stop_of(&e))?;
    loop {
        match call {
            TypedResumableCall::Finished(result) => {
                // what the slice had left goes back
                *left += store.get_fuel().unwrap_or(0);
                return Ok(result);
            }
            TypedResumableCall::HostTrap(_) => return Err(Stop::Crashed),
            TypedResumableCall::OutOfFuel(paused) => {
                let need = paused.required_fuel();
                if need > *left || *left == 0 || Instant::now() >= until {
                    return Err(Stop::TooSlow);
                }
                let give = (*left).min(FUEL_SLICE.max(need));
                *left -= give;
                store.set_fuel(give).map_err(|_| Stop::Damaged)?;
                call = paused.resume(&mut *store).map_err(|e| stop_of(&e))?;
            }
        }
    }
}

/// A plugin's module, instantiated, with what it has of ours to call.
pub(crate) struct Running {
    store: Store<StoreLimits>,
    position: Option<TypedFunc<(f32, f32), i64>>,
    pick_layer: Option<TypedFunc<(), i32>>,
    interface_scale: Option<TypedFunc<(f32, f32), f32>>,
    paste_mask: Option<TypedFunc<(), i32>>,
    row_buffer: Option<TypedFunc<i32, i32>>,
    paste_row: Option<TypedFunc<(i32, i32), ()>>,
    fill: Option<FillCalls>,
    fill_all_layers: Option<TypedFunc<(), i32>>,
    radial_segments: Option<TypedFunc<(), i32>>,
    hide_undo_notice: Option<TypedFunc<(), i32>>,
    interface_colour: Option<TypedFunc<i32, i32>>,
    pin_panels: Option<TypedFunc<(), i32>>,
    opacity_together: Option<TypedFunc<(), i32>>,
    randomise: Option<TypedFunc<(i32, i32), f32>>,
    colour_triangle: Option<TypedFunc<(), i32>>,
    curve: Option<CurveCalls>,
    stroke_sound: Option<SoundCall>,
    sound_prepare: Option<TypedFunc<i32, i32>>,
    text_buffer: Option<TypedFunc<i32, i32>>,
    brush: Option<TypedFunc<(i32, i32), ()>>,
    brush_control: Option<TypedFunc<(i32, f32), ()>>,
    file_buffer: Option<TypedFunc<(i32, i32), i32>>,
    file: Option<TypedFunc<(i32, i32), ()>>,
    /// The rate its sounds are readied for, and the brush it was last told (as a number).
    sound_rate: Option<i32>,
    told_brush: Option<u64>,
    between: Option<BetweenCalls>,
    memory: Option<Memory>,
    control: Option<TypedFunc<(i32, f32), ()>>,
}

impl Running {
    /// `bytes` started, under the caps of a plugin on `hooks`, and told `controls`.
    pub(crate) fn start_on(bytes: &[u8], controls: &[f32], hooks: &[Hook]) -> Result<Running, Stop> {
        let most_memory = if hooks.contains(&Hook::Inbetween) {
            MOST_BETWEEN_MEMORY
        } else if hooks.contains(&Hook::StrokeSound) {
            MOST_SOUND_MEMORY
        } else if hooks.contains(&Hook::FillMask) {
            MOST_FILL_MEMORY
        } else {
            MOST_MEMORY
        };
        Running::start_within(bytes, controls, most_memory)
    }

    /// `bytes` started, under its caps, and told `controls`.
    #[cfg(test)]
    pub(crate) fn start(bytes: &[u8], controls: &[f32]) -> Result<Running, Stop> {
        Running::start_within(bytes, controls, MOST_MEMORY)
    }

    /// How much memory it has come to ask for, bytes.
    #[cfg(test)]
    pub(crate) fn memory_used(&self) -> usize {
        self.memory.as_ref().map_or(0, |memory| memory.data(&self.store).len())
    }

    fn start_within(bytes: &[u8], controls: &[f32], most_memory: usize) -> Result<Running, Stop> {
        let mut running = guarded(|| {
            let engine = engine();
            let module = Module::new(&engine, bytes).map_err(|_| Stop::Damaged)?;
            let limits = StoreLimitsBuilder::new()
                .memory_size(most_memory)
                .table_elements(MOST_TABLE)
                .instances(1)
                .memories(1)
                .tables(1)
                .trap_on_grow_failure(true)
                .build();
            let mut store = Store::new(&engine, limits);
            store.limiter(|limits| limits);
            store.set_fuel(START_FUEL).map_err(|_| Stop::Damaged)?;
            let linker = Linker::new(&engine);
            let instance = linker.instantiate_and_start(&mut store, &module).map_err(|e| stop_of(&e))?;
            let position = instance.get_typed_func(&store, POSITION).ok();
            let pick_layer = instance.get_typed_func(&store, PICK_LAYER).ok();
            let interface_scale = instance.get_typed_func(&store, INTERFACE_SCALE).ok();
            let paste_mask = instance.get_typed_func(&store, PASTE_MASK).ok();
            let row_buffer = instance.get_typed_func(&store, ROW_BUFFER).ok();
            let paste_row = instance.get_typed_func(&store, PASTE_ROW).ok();
            let fill = (|| {
                Some(FillCalls {
                    mask: instance.get_typed_func(&store, FILL_MASK).ok()?,
                    begin: instance.get_typed_func(&store, FILL_BEGIN).ok()?,
                    row: instance.get_typed_func(&store, FILL_ROW).ok()?,
                    run: instance.get_typed_func(&store, FILL_RUN).ok()?,
                    out: instance.get_typed_func(&store, FILL_OUT).ok()?,
                })
            })();
            let fill_all_layers = instance.get_typed_func(&store, FILL_ALL_LAYERS).ok();
            let radial_segments = instance.get_typed_func(&store, RADIAL_SEGMENTS).ok();
            let hide_undo_notice = instance.get_typed_func(&store, HIDE_UNDO_NOTICE).ok();
            let interface_colour = instance.get_typed_func(&store, INTERFACE_COLOUR).ok();
            let pin_panels = instance.get_typed_func(&store, PIN_PANELS).ok();
            let opacity_together = instance.get_typed_func(&store, OPACITY_TOGETHER).ok();
            let randomise = instance.get_typed_func(&store, RANDOMISE).ok();
            let colour_triangle = instance.get_typed_func(&store, COLOUR_TRIANGLE).ok();
            let curve = (|| {
                Some(CurveCalls {
                    begin: instance.get_typed_func(&store, CURVE_BEGIN).ok()?,
                    run: instance.get_typed_func(&store, CURVE_RUN).ok()?,
                    nodes: instance.get_typed_func(&store, CURVE_NODES).ok()?,
                })
            })();
            let stroke_sound = instance.get_typed_func(&store, STROKE_SOUND).ok();
            let sound_prepare = instance.get_typed_func(&store, SOUND_PREPARE).ok();
            let text_buffer = instance.get_typed_func(&store, TEXT_BUFFER).ok();
            let brush = instance.get_typed_func(&store, BRUSH).ok();
            let brush_control = instance.get_typed_func(&store, BRUSH_CONTROL).ok();
            let file_buffer = instance.get_typed_func(&store, FILE_BUFFER).ok();
            let file = instance.get_typed_func(&store, FILE).ok();
            let between = (|| {
                Some(BetweenCalls {
                    begin: instance.get_typed_func(&store, BETWEEN_BEGIN).ok()?,
                    row: instance.get_typed_func(&store, BETWEEN_ROW).ok()?,
                    run: instance.get_typed_func(&store, BETWEEN_RUN).ok()?,
                    out: instance.get_typed_func(&store, BETWEEN_OUT).ok()?,
                })
            })();
            let memory = instance.get_memory(&store, MEMORY);
            let control = instance.get_typed_func(&store, CONTROL).ok();
            Ok(Running {
                store,
                position,
                pick_layer,
                interface_scale,
                paste_mask,
                row_buffer,
                paste_row,
                fill,
                fill_all_layers,
                radial_segments,
                hide_undo_notice,
                interface_colour,
                pin_panels,
                opacity_together,
                randomise,
                colour_triangle,
                curve,
                stroke_sound,
                sound_prepare,
                text_buffer,
                brush,
                brush_control,
                file_buffer,
                file,
                sound_rate: None,
                told_brush: None,
                between,
                memory,
                control,
            })
        })?;
        for (index, value) in controls.iter().enumerate() {
            running.control(index, *value)?;
        }
        Ok(running)
    }

    /// Tells it control `index` has `value`.
    pub(crate) fn control(&mut self, index: usize, value: f32) -> Result<(), Stop> {
        let Running { store, control, .. } = self;
        let Some(control) = control else { return Ok(()) };
        guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            control.call(&mut *store, (index as i32, value)).map_err(|e| stop_of(&e))
        })
    }

    /// Whether it says the eyedropper reads the current layer.
    pub(crate) fn pick_layer(&mut self) -> Result<bool, Stop> {
        let Running { store, pick_layer, .. } = self;
        let Some(pick_layer) = pick_layer else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            pick_layer.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// Whether it says ColorDrop fills by every visible layer.
    pub(crate) fn fill_all_layers(&mut self) -> Result<bool, Stop> {
        let Running { store, fill_all_layers, .. } = self;
        let Some(fill_all_layers) = fill_all_layers else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            fill_all_layers.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// The colour (0xRRGGBB) it says the interface draws `role` in; none if it leaves it be (-1).
    pub(crate) fn interface_colour(&mut self, role: i32) -> Result<Option<u32>, Stop> {
        let Running { store, interface_colour, .. } = self;
        let Some(interface_colour) = interface_colour else { return Ok(None) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            interface_colour.call(&mut *store, role).map_err(|e| stop_of(&e))
        })?;
        match said {
            -1 => Ok(None),
            0..=0xFF_FFFF => Ok(u32::try_from(said).ok()),
            _ => Err(Stop::Nonsense),
        }
    }

    /// The frame between `this` and `next` (each `width` × `height`, premultiplied RGBA8), as it
    /// draws it: a long call, on fuel by the area. None without the hook, or if it draws none.
    pub(crate) fn inbetween(
        &mut self,
        this: &[u8],
        next: &[u8],
        width: usize,
        height: usize,
    ) -> Result<Option<Vec<u8>>, Stop> {
        let Running { store, between, memory, .. } = self;
        let (Some(between), Some(memory)) = (between.as_ref(), memory.as_ref()) else { return Ok(None) };
        let row = width * 4;
        if row == 0 || height == 0 || this.len() != row * height || next.len() != row * height {
            return Ok(None);
        }
        let until = Instant::now() + MOST_FILL_TIME;
        let mut left = BETWEEN_FUEL + BETWEEN_PIXEL_FUEL * (width * height) as u64;
        guarded(|| {
            let size =
                (i32::try_from(width).map_err(|_| Stop::Nonsense)?, i32::try_from(height).map_err(|_| Stop::Nonsense)?);
            let at = long_call(store, &between.begin, size, &mut left, until)?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            for (frame, pixels) in [this, next].into_iter().enumerate() {
                for (y, line) in pixels.chunks_exact(row).enumerate() {
                    memory.write(&mut *store, place, line).map_err(|_| Stop::Nonsense)?;
                    long_call(store, &between.row, (frame as i32, y as i32), &mut left, until)?;
                }
            }
            if long_call(store, &between.run, (), &mut left, until)? == 0 {
                return Ok(None);
            }
            let mut out = vec![0_u8; row * height];
            for (y, line) in out.chunks_exact_mut(row).enumerate() {
                long_call(store, &between.out, y as i32, &mut left, until)?;
                memory.read(&*store, place, line).map_err(|_| Stop::Nonsense)?;
            }
            Ok(Some(out))
        })
    }

    /// The sound of the stroke as `motion` has it: `frames` samples at `rate` a second, as it makes
    /// them, readied for the rate first (a long call, the first time at it) and told `brush` when
    /// that changed. None without the hook.
    pub(crate) fn stroke_sound(
        &mut self,
        frames: usize,
        rate: u32,
        motion: &crate::stroke_sound::SoundMotion,
        brush: &crate::stroke_sound::SoundBrush,
    ) -> Result<Option<Vec<f32>>, Stop> {
        if self.stroke_sound.is_none() || self.memory.is_none() {
            return Ok(None);
        }
        let rate = i32::try_from(rate).map_err(|_| Stop::Nonsense)?;
        if self.sound_rate != Some(rate) {
            self.sound_prepare(rate)?;
            self.sound_rate = Some(rate);
        }
        if self.told_brush != Some(brush.key) {
            self.tell_brush(brush)?;
            self.told_brush = Some(brush.key);
        }
        let Running { store, stroke_sound, memory, .. } = self;
        let (Some(call), Some(memory)) = (stroke_sound.as_ref(), memory.as_ref()) else { return Ok(None) };
        let count = i32::try_from(frames).map_err(|_| Stop::Nonsense)?;
        let m = motion;
        let args = (count, rate, m.speed, m.pressure, m.since_ms, m.state, m.hover.0, m.hover.1, m.height);
        let at = guarded(|| {
            store.set_fuel(SOUND_FUEL + SOUND_FRAME_FUEL * frames as u64).map_err(|_| Stop::Damaged)?;
            call.call(&mut *store, args).map_err(|e| stop_of(&e))
        })?;
        let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
        let mut bytes = vec![0_u8; frames * 4];
        memory.read(&*store, place, &mut bytes).map_err(|_| Stop::Nonsense)?;
        let sample = |b: &[u8; 4]| {
            let s = f32::from_le_bytes(*b);
            if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }
        };
        Ok(Some(bytes.as_chunks::<4>().0.iter().map(sample).collect()))
    }

    /// Readies its sounds for `rate`, if it asks to be: a long call.
    fn sound_prepare(&mut self, rate: i32) -> Result<(), Stop> {
        let Running { store, sound_prepare, .. } = self;
        let Some(prepare) = sound_prepare.as_ref() else { return Ok(()) };
        let (mut left, until) = (PREPARE_FUEL, Instant::now() + MOST_FILL_TIME);
        guarded(|| long_call(store, prepare, rate, &mut left, until).map(|_| ()))
    }

    /// Tells it the current brush, if it listens: its name and set, and its rows in the brush studio.
    fn tell_brush(&mut self, brush: &crate::stroke_sound::SoundBrush) -> Result<(), Stop> {
        let Running { store, text_buffer, brush: named, brush_control, memory, .. } = self;
        if let (Some(text_buffer), Some(named), Some(memory)) = (text_buffer.as_ref(), named.as_ref(), memory.as_ref())
        {
            let text = [brush.name.as_bytes(), brush.set.as_bytes()].concat();
            let length = |n: usize| i32::try_from(n).map_err(|_| Stop::Nonsense);
            guarded(|| {
                store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
                let at = text_buffer.call(&mut *store, length(text.len())?).map_err(|e| stop_of(&e))?;
                let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
                memory.write(&mut *store, place, &text).map_err(|_| Stop::Nonsense)?;
                store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
                named.call(&mut *store, (length(brush.name.len())?, length(brush.set.len())?)).map_err(|e| stop_of(&e))
            })?;
        }
        if let Some(brush_control) = brush_control.as_ref() {
            for (index, value) in brush.controls.iter().enumerate() {
                let index = i32::try_from(index).map_err(|_| Stop::Nonsense)?;
                guarded(|| {
                    store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
                    brush_control.call(&mut *store, (index, *value)).map_err(|e| stop_of(&e))
                })?;
            }
        }
        Ok(())
    }

    /// Hands it the file picked for its control `index` (none: let go), if it takes them: a long
    /// call.
    pub(crate) fn give_file(&mut self, index: usize, bytes: Option<&[u8]>) -> Result<(), Stop> {
        let Running { store, file_buffer, file, memory, .. } = self;
        let (Some(file), Some(memory)) = (file.as_ref(), memory.as_ref()) else { return Ok(()) };
        let data = bytes.unwrap_or(&[]);
        let index = i32::try_from(index).map_err(|_| Stop::Nonsense)?;
        let len = i32::try_from(data.len()).map_err(|_| Stop::Nonsense)?;
        let (mut left, until) = (FILE_FUEL, Instant::now() + MOST_FILL_TIME);
        guarded(|| {
            if !data.is_empty() {
                let Some(file_buffer) = file_buffer.as_ref() else { return Ok(()) };
                let at = long_call(store, file_buffer, (index, len), &mut left, until)?;
                let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
                memory.write(&mut *store, place, data).map_err(|_| Stop::Nonsense)?;
            }
            long_call(store, file, (index, len), &mut left, until)
        })
    }

    /// The curve it offers for a snapping stroke of `points` (window pixels), QuickShape having
    /// found `best` first (-1: a curve asked for): its nodes, an anchor then two controls and an
    /// anchor for each span. None without the hook, or if it offers none.
    pub(crate) fn quickshape_curve(
        &mut self,
        points: &[(f32, f32)],
        best: i32,
    ) -> Result<Option<Vec<(f32, f32)>>, Stop> {
        let Running { store, curve, memory, .. } = self;
        let (Some(curve), Some(memory)) = (curve.as_ref(), memory.as_ref()) else { return Ok(None) };
        let count = i32::try_from(points.len()).map_err(|_| Stop::Nonsense)?;
        let until = Instant::now() + Duration::from_secs(2);
        let mut left = CURVE_FUEL + CURVE_POINT_FUEL * points.len() as u64;
        guarded(|| {
            let at = long_call(store, &curve.begin, (count, best), &mut left, until)?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            let bytes: Vec<u8> =
                points.iter().flat_map(|(x, y)| [x.to_le_bytes(), y.to_le_bytes()]).flatten().collect();
            memory.write(&mut *store, place, &bytes).map_err(|_| Stop::Nonsense)?;
            let n = usize::try_from(long_call(store, &curve.run, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            if n == 0 {
                return Ok(None);
            }
            if n < 4 || n % 3 != 1 || n > MOST_CURVE_NODES {
                return Err(Stop::Nonsense);
            }
            let at = long_call(store, &curve.nodes, (), &mut left, until)?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            let mut bytes = vec![0_u8; n * 8];
            memory.read(&*store, place, &mut bytes).map_err(|_| Stop::Nonsense)?;
            let nodes: Vec<(f32, f32)> = bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| (f32::from_le_bytes([b[0], b[1], b[2], b[3]]), f32::from_le_bytes([b[4], b[5], b[6], b[7]])))
                .collect();
            let near = |v: f32| v.is_finite() && v.abs() <= MOST_MOVE * 4.0;
            if nodes.iter().all(|(x, y)| near(*x) && near(*y)) { Ok(Some(nodes)) } else { Err(Stop::Nonsense) }
        })
    }

    /// Whether it says Classic shows a triangle.
    pub(crate) fn colour_triangle(&mut self) -> Result<bool, Stop> {
        let Running { store, colour_triangle, .. } = self;
        let Some(colour_triangle) = colour_triangle else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            colour_triangle.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// Where it says brush setting `setting` goes, as a share of its range (none: left as it is).
    pub(crate) fn randomise(&mut self, setting: i32, seed: i32) -> Result<Option<f32>, Stop> {
        let Running { store, randomise, .. } = self;
        let Some(randomise) = randomise else { return Ok(None) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            randomise.call(&mut *store, (setting, seed)).map_err(|e| stop_of(&e))
        })?;
        if said.is_nan() || said > 1.0 {
            Err(Stop::Nonsense)
        } else if said < 0.0 {
            Ok(None)
        } else {
            Ok(Some(said))
        }
    }

    /// Whether it says picked layers' opacity changes together.
    pub(crate) fn opacity_together(&mut self) -> Result<bool, Stop> {
        let Running { store, opacity_together, .. } = self;
        let Some(opacity_together) = opacity_together else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            opacity_together.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// Whether it says panels stay open while the canvas is painted.
    pub(crate) fn pin_panels(&mut self) -> Result<bool, Stop> {
        let Running { store, pin_panels, .. } = self;
        let Some(pin_panels) = pin_panels else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            pin_panels.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// Whether it says undo and redo go by without their notice.
    pub(crate) fn hide_undo_notice(&mut self) -> Result<bool, Stop> {
        let Running { store, hide_undo_notice, .. } = self;
        let Some(hide_undo_notice) = hide_undo_notice else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            hide_undo_notice.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// How many parts it says radial symmetry makes (none if it has no say).
    pub(crate) fn radial_segments(&mut self) -> Result<Option<u32>, Stop> {
        let Running { store, radial_segments, .. } = self;
        let Some(radial_segments) = radial_segments else { return Ok(None) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            radial_segments.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        if RADIAL_PARTS.contains(&said) { Ok(u32::try_from(said).ok()) } else { Err(Stop::Nonsense) }
    }

    /// How big it says the interface shows in a window `width` × `height` points at 1.
    pub(crate) fn interface_scale(&mut self, width: f32, height: f32) -> Result<f32, Stop> {
        let Running { store, interface_scale, .. } = self;
        let Some(interface_scale) = interface_scale else { return Ok(1.0) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            interface_scale.call(&mut *store, (width, height)).map_err(|e| stop_of(&e))
        })?;
        if SCALES.contains(&said) { Ok(said) } else { Err(Stop::Nonsense) }
    }

    /// Whether it says a picture pasted with a mask current goes into the mask.
    pub(crate) fn paste_mask(&mut self) -> Result<bool, Stop> {
        let Running { store, paste_mask, .. } = self;
        let Some(paste_mask) = paste_mask else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            paste_mask.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// `rgba`, rows of `width` pixels, through it a row at a time: a picture is too big for its
    /// memory, a row never is, and each gets fuel by its width. Part of it may have gone through
    /// when it stops.
    pub(crate) fn paste_rows(&mut self, rgba: &mut [u8], width: usize) -> Result<(), Stop> {
        let Running { store, row_buffer, paste_row, memory, .. } = self;
        let (Some(row_buffer), Some(paste_row), Some(memory)) = (row_buffer, paste_row, memory) else {
            return Err(Stop::Damaged);
        };
        let bytes = width * 4;
        guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            let at = row_buffer.call(&mut *store, bytes as i32).map_err(|e| stop_of(&e))?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            let fuel = ROW_FUEL + PIXEL_FUEL * width as u64;
            for row in rgba.chunks_exact_mut(bytes.max(1)) {
                memory.write(&mut *store, place, row).map_err(|_| Stop::Nonsense)?;
                store.set_fuel(fuel).map_err(|_| Stop::Damaged)?;
                paste_row.call(&mut *store, (at, width as i32)).map_err(|e| stop_of(&e))?;
                memory.read(&*store, place, row).map_err(|_| Stop::Nonsense)?;
            }
            Ok(())
        })
    }

    /// Whether it says a fill being let go has anything to go through.
    pub(crate) fn fill_mask(&mut self) -> Result<bool, Stop> {
        let Running { store, fill, .. } = self;
        let Some(fill) = fill else { return Ok(false) };
        let said = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            fill.mask.call(&mut *store, ()).map_err(|e| stop_of(&e))
        })?;
        Ok(said != 0)
    }

    /// `rows`, a fill of `width` × `height` from `seed`, a bit a pixel in words of 64, through it: a
    /// long call, on fuel by its area. What comes out comes out of `rows`, which is no use if it
    /// stops. Whether anything did.
    pub(crate) fn fill(
        &mut self,
        rows: &mut [u64],
        width: usize,
        height: usize,
        seed: (usize, usize),
    ) -> Result<bool, Stop> {
        let Running { store, fill, memory, .. } = self;
        let (Some(fill), Some(memory)) = (fill.as_ref(), memory.as_ref()) else { return Ok(false) };
        let words = width.div_ceil(64).max(1);
        let until = Instant::now() + MOST_FILL_TIME;
        let mut left = FILL_FUEL + FILL_PIXEL_FUEL * (width * height) as u64;
        guarded(|| {
            let size = (width as i32, height as i32, seed.0 as i32, seed.1 as i32);
            let at = long_call(store, &fill.begin, size, &mut left, until)?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            let mut bytes = vec![0_u8; words * 8];
            for (y, row) in rows.chunks_exact(words).enumerate() {
                for (to, word) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(row) {
                    *to = word.to_le_bytes();
                }
                memory.write(&mut *store, place, &bytes).map_err(|_| Stop::Nonsense)?;
                long_call(store, &fill.row, y as i32, &mut left, until)?;
            }
            if long_call(store, &fill.run, (), &mut left, until)? == 0 {
                return Ok(false);
            }
            for (y, row) in rows.chunks_exact_mut(words).enumerate() {
                long_call(store, &fill.out, y as i32, &mut left, until)?;
                memory.read(&*store, place, &mut bytes).map_err(|_| Stop::Nonsense)?;
                for (word, from) in row.iter_mut().zip(bytes.as_chunks::<8>().0) {
                    *word &= !u64::from_le_bytes(*from);
                }
            }
            let kept = rows.get(seed.1 * words + seed.0 / 64).is_some_and(|word| word >> (seed.0 % 64) & 1 == 1);
            if kept { Ok(true) } else { Err(Stop::Nonsense) }
        })
    }

    /// Where it says a sample at (x, y) lands.
    pub(crate) fn position(&mut self, x: f32, y: f32) -> Result<(f32, f32), Stop> {
        let Running { store, position, .. } = self;
        let Some(position) = position else { return Ok((x, y)) };
        let packed = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            position.call(&mut *store, (x, y)).map_err(|e| stop_of(&e))
        })? as u64;
        let (to_x, to_y) = (f32::from_bits(packed as u32), f32::from_bits((packed >> 32) as u32));
        let near = |to: f32, from: f32| to.is_finite() && (to - from).abs() <= MOST_MOVE;
        if near(to_x, x) && near(to_y, y) { Ok((to_x, to_y)) } else { Err(Stop::Nonsense) }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Moves a sample by its two controls, as a plugin of real use does.
    pub(crate) const OFFSET: &str = r#"(module
        (global $x (mut f32) (f32.const 0))
        (global $y (mut f32) (f32.const 0))
        (func (export "procraft_control") (param i32 f32)
            (if (i32.eqz (local.get 0))
                (then (global.set $x (local.get 1)))
                (else (global.set $y (local.get 1)))))
        (func (export "procraft_position") (param f32 f32) (result i64)
            (i64.or
                (i64.extend_i32_u (i32.reinterpret_f32 (f32.add (local.get 0) (global.get $x))))
                (i64.shl
                    (i64.extend_i32_u (i32.reinterpret_f32 (f32.add (local.get 1) (global.get $y))))
                    (i64.const 32)))))"#;

    /// A module whose `procraft_position` does `body`, after what `before` declares.
    pub(crate) fn position_module(before: &str, body: &str) -> Vec<u8> {
        let text =
            format!(r#"(module {before} (func (export "procraft_position") (param f32 f32) (result i64) {body}))"#);
        wat::parse_str(text).unwrap()
    }

    fn started(bytes: &[u8]) -> Running {
        Running::start(bytes, &[]).map_err(|stop| format!("{stop:?}")).unwrap()
    }

    #[test]
    fn a_plugin_moves_a_sample_by_what_its_controls_say() {
        let bytes = wat::parse_str(OFFSET).unwrap();
        assert!(can_run(&bytes, &[Hook::Position], true));
        let mut running = Running::start(&bytes, &[3.0, -2.0]).unwrap();
        assert_eq!(running.position(10.0, 20.0), Ok((13.0, 18.0)));
        running.control(1, 5.0).unwrap();
        assert_eq!(running.position(10.0, 20.0), Ok((13.0, 25.0)));
    }

    #[test]
    fn what_a_module_wants_from_outside_keeps_it_out() {
        let wants = |import: &str| {
            let text = format!(
                r#"(module {import}
                    (func (export "procraft_position") (param f32 f32) (result i64) (i64.const 0)))"#
            );
            can_run(&wat::parse_str(text).unwrap(), &[Hook::Position], false)
        };
        assert!(wants(""));
        assert!(!wants(r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#));
        assert!(!wants(r#"(import "env" "memory" (memory 1))"#));
        assert!(!wants(r#"(import "env" "g" (global i32))"#));
    }

    #[test]
    fn a_module_must_export_what_it_is_called_by() {
        let refused = |text: &str, controls| !can_run(&wat::parse_str(text).unwrap(), &[Hook::Position], controls);
        assert!(refused("(module)", false));
        // not a function, or not the function it must be
        assert!(refused(r#"(module (memory (export "procraft_position") 1))"#, false));
        assert!(refused(
            r#"(module (func (export "procraft_position") (param f32) (result i64) (i64.const 0)))"#,
            false
        ));
        assert!(refused(
            r#"(module (func (export "procraft_position") (param f32 f32) (result f32) (f32.const 0)))"#,
            false
        ));
        // controls with nothing to tell them to
        let no_control = r#"(module (func (export "procraft_position") (param f32 f32) (result i64) (i64.const 0)))"#;
        assert!(!refused(no_control, false));
        assert!(refused(no_control, true));
    }

    #[test]
    fn a_module_too_big_or_not_a_module_is_refused() {
        assert!(!can_run(b"", &[Hook::Position], false));
        assert!(!can_run(b"\0asm", &[Hook::Position], false));
        assert!(!can_run(b"not a module at all", &[Hook::Position], false));
        let mut big = position_module("", "(i64.const 0)");
        assert!(can_run(&big, &[Hook::Position], false));
        big.resize(MOST_MODULE + 1, 0);
        assert!(!can_run(&big, &[Hook::Position], false));
    }

    #[test]
    fn a_code_section_of_no_functions_with_bytes_in_it_is_refused() {
        // the case that panicked the limits, with and without a second code section after it
        let crafted = [0, b'a', b's', b'm', 1, 0, 0, 0, 10, 3, 0, 0, 0];
        assert!(!code_counts_hold(&crafted));
        assert!(!can_run(&crafted, &[Hook::Position], false));
        let twice = [0, b'a', b's', b'm', 1, 0, 0, 0, 10, 1, 0, 10, 3, 0, 0, 0];
        assert!(!can_run(&twice, &[Hook::Position], false));
        assert!(code_counts_hold(&position_module("", "(i64.const 0)")));
    }

    #[test]
    fn no_damage_to_a_module_panics_the_loader() {
        let good = wat::parse_str(OFFSET).unwrap();
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..3000 {
            let mut bytes = good.clone();
            for _ in 0..1 + next() % 8 {
                let at = next() as usize % bytes.len();
                bytes[at] = next() as u8;
            }
            bytes.truncate(bytes.len() - next() as usize % 8);
            // whatever it makes of it, and whatever it does if it still loads
            if can_run(&bytes, &[Hook::Position], true)
                && let Ok(mut running) = Running::start(&bytes, &[1.0, 2.0])
            {
                let _ = running.position(10.0, 20.0);
            }
        }
    }

    #[test]
    fn a_hook_needs_its_export_and_a_module_answers_only_what_it_exports() {
        let picks =
            wat::parse_str(r#"(module (func (export "procraft_pick_layer") (result i32) (i32.const 7)))"#).unwrap();
        assert!(can_run(&picks, &[Hook::PickLayer], false));
        assert!(!can_run(&picks, &[Hook::Position], false));
        assert!(!can_run(&picks, &[Hook::PickLayer, Hook::Position], false));
        let mut running = started(&picks);
        assert_eq!(running.pick_layer(), Ok(true));
        // what it doesn't export goes on as it came
        assert_eq!(running.position(1.0, 2.0), Ok((1.0, 2.0)));
        let wrong = r#"(module (func (export "procraft_pick_layer") (param i32) (result i32) (i32.const 1)))"#;
        assert!(!can_run(&wat::parse_str(wrong).unwrap(), &[Hook::PickLayer], false));
    }

    #[test]
    fn a_scale_nobody_could_use_is_refused() {
        let says = |value: &str| {
            let text = format!(
                r#"(module (func (export "procraft_interface_scale") (param f32 f32) (result f32) (f32.const {value})))"#
            );
            started(&wat::parse_str(text).unwrap()).interface_scale(800.0, 600.0)
        };
        assert_eq!(says("1.25"), Ok(1.25));
        assert_eq!(says("0.5"), Ok(0.5));
        assert_eq!(says("2"), Ok(2.0));
        for bad in ["0", "-1", "0.49", "2.01", "nan", "inf"] {
            assert_eq!(says(bad), Err(Stop::Nonsense), "{bad}");
        }
    }

    /// A paste module whose `procraft_paste_row` does `body`, its buffer where `buffer` says.
    pub(crate) fn paste_module(buffer: &str, body: &str) -> Vec<u8> {
        let text = format!(
            r#"(module (memory (export "memory") 2)
                (func (export "procraft_paste_mask") (result i32) (i32.const 1))
                (func (export "procraft_row_buffer") (param i32) (result i32) {buffer})
                (func (export "procraft_paste_row") (param $at i32) (param $width i32) {body}))"#
        );
        wat::parse_str(text).unwrap()
    }

    #[test]
    fn a_picture_goes_through_a_row_at_a_time_and_comes_back() {
        // each row's first byte set to its width, to show it went in and came back
        let bytes = paste_module("(i32.const 65536)", "(i32.store8 (local.get $at) (local.get $width))");
        assert!(can_run(&bytes, &[Hook::PasteMask], false));
        let mut running = started(&bytes);
        assert_eq!(running.paste_mask(), Ok(true));
        let mut rgba = vec![9_u8; 3 * 2 * 4];
        assert_eq!(running.paste_rows(&mut rgba, 3), Ok(()));
        assert_eq!(rgba[..14], [3, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 3, 9]);
    }

    #[test]
    fn the_paste_hook_needs_its_memory_and_all_its_exports() {
        let good = paste_module("(i32.const 65536)", "");
        assert!(can_run(&good, &[Hook::PasteMask], false));
        for wrong in [
            r#"(module (func (export "procraft_paste_mask") (result i32) (i32.const 1)))"#,
            r#"(module (memory 1)
                (func (export "procraft_paste_mask") (result i32) (i32.const 1))
                (func (export "procraft_row_buffer") (param i32) (result i32) (i32.const 0))
                (func (export "procraft_paste_row") (param i32 i32)))"#,
        ] {
            assert!(!can_run(&wat::parse_str(wrong).unwrap(), &[Hook::PasteMask], false));
        }
    }

    #[test]
    fn a_row_put_nowhere_stops_it() {
        let mut rgba = vec![0_u8; 16];
        for buffer in ["(i32.const -4)", "(i32.const 1000000)", "(i32.const 131070)"] {
            let mut running = started(&paste_module(buffer, ""));
            assert_eq!(running.paste_rows(&mut rgba, 4), Err(Stop::Nonsense), "{buffer}");
        }
    }

    #[test]
    fn a_row_that_never_ends_runs_out_of_fuel() {
        let mut rgba = vec![0_u8; 16];
        let mut running = started(&paste_module("(i32.const 65536)", "(loop (br 0))"));
        assert_eq!(running.paste_rows(&mut rgba, 4), Err(Stop::TooSlow));
    }

    #[test]
    fn a_row_that_crashes_stops_it() {
        let mut rgba = vec![0_u8; 16];
        let mut running = started(&paste_module("(i32.const 65536)", "(unreachable)"));
        assert_eq!(running.paste_rows(&mut rgba, 4), Err(Stop::Crashed));
    }

    /// A fill module: `begin` says where rows go, `run` and `out` do what they are given.
    pub(crate) fn fill_module(begin: &str, run: &str, out: &str) -> Vec<u8> {
        let text = format!(
            r#"(module (memory (export "memory") 2)
                (func (export "procraft_fill_mask") (result i32) (i32.const 1))
                (func (export "procraft_fill_begin") (param i32 i32 i32 i32) (result i32) {begin})
                (func (export "procraft_fill_row") (param i32))
                (func (export "procraft_fill_run") (result i32) {run})
                (func (export "procraft_fill_out") (param $y i32) {out}))"#
        );
        wat::parse_str(text).unwrap()
    }

    pub(crate) const FILL_AT: &str = "(i32.const 65536)";
    /// Takes out all of each row's first word but its lowest bit.
    pub(crate) const TAKES_MOST: &str = "(i64.store (i32.const 65536) (i64.const -2))";

    #[test]
    fn a_fill_goes_through_a_row_at_a_time_and_only_loses_pixels() {
        let bytes = fill_module(FILL_AT, "(i32.const 1)", TAKES_MOST);
        assert!(can_run(&bytes, &[Hook::FillMask], false));
        let mut running = started(&bytes);
        assert_eq!(running.fill_mask(), Ok(true));
        // two rows, a word each: what wasn't in the fill doesn't come in
        let mut rows = vec![0b1011_u64, 0b0100];
        assert_eq!(running.fill(&mut rows, 64, 2, (0, 0)), Ok(true));
        assert_eq!(rows, [1, 0]);
    }

    #[test]
    fn a_fill_nothing_comes_out_of_stays_as_it_was() {
        let bytes = fill_module(FILL_AT, "(i32.const 0)", TAKES_MOST);
        let mut rows = vec![0b1011_u64];
        assert_eq!(started(&bytes).fill(&mut rows, 64, 1, (0, 0)), Ok(false));
        assert_eq!(rows, [0b1011]);
    }

    #[test]
    fn the_fill_hook_needs_its_memory_and_all_its_exports() {
        let good = fill_module(FILL_AT, "(i32.const 0)", "");
        assert!(can_run(&good, &[Hook::FillMask], false));
        let no_memory = r#"(module
            (func (export "procraft_fill_mask") (result i32) (i32.const 1))
            (func (export "procraft_fill_begin") (param i32 i32 i32 i32) (result i32) (i32.const 0))
            (func (export "procraft_fill_row") (param i32))
            (func (export "procraft_fill_run") (result i32) (i32.const 0))
            (func (export "procraft_fill_out") (param i32)))"#;
        assert!(!can_run(&wat::parse_str(no_memory).unwrap(), &[Hook::FillMask], false));
        let one_short = r#"(module (memory (export "memory") 1)
            (func (export "procraft_fill_mask") (result i32) (i32.const 1)))"#;
        assert!(!can_run(&wat::parse_str(one_short).unwrap(), &[Hook::FillMask], false));
    }

    #[test]
    fn a_fill_that_goes_wrong_says_how() {
        let says = |begin: &str, run: &str, out: &str| {
            let mut rows = vec![!0_u64; 4];
            started(&fill_module(begin, run, out)).fill(&mut rows, 64, 4, (0, 0))
        };
        assert_eq!(says(FILL_AT, "(loop (br 0)) (i32.const 1)", ""), Err(Stop::TooSlow));
        assert_eq!(says(FILL_AT, "(unreachable)", ""), Err(Stop::Crashed));
        assert_eq!(says("(unreachable)", "(i32.const 1)", ""), Err(Stop::Crashed));
        assert_eq!(says(FILL_AT, "(i32.const 1)", "(unreachable)"), Err(Stop::Crashed));
        // rows put nowhere
        assert_eq!(says("(i32.const -8)", "(i32.const 1)", ""), Err(Stop::Nonsense));
        assert_eq!(says("(i32.const 1000000)", "(i32.const 1)", ""), Err(Stop::Nonsense));
        // what it can't be let do: take the seed itself
        assert_eq!(says(FILL_AT, "(i32.const 1)", "(i64.store (i32.const 65536) (i64.const -1))"), Err(Stop::Nonsense));
    }

    #[test]
    fn a_fill_gets_more_memory_than_other_hooks_and_no_more_than_that() {
        let grows =
            |pages: usize| fill_module(FILL_AT, &format!("(drop (memory.grow (i32.const {pages}))) (i32.const 0)"), "");
        let fits = MOST_MEMORY / 65536 + 16;
        let over = MOST_FILL_MEMORY / 65536;
        let run = |bytes: &[u8], hooks: &[Hook]| {
            let mut rows = vec![!0_u64];
            Running::start_on(bytes, &[], hooks).unwrap().fill(&mut rows, 64, 1, (0, 0))
        };
        assert_eq!(run(&grows(fits), &[Hook::FillMask]), Ok(false));
        assert_eq!(run(&grows(over), &[Hook::FillMask]), Err(Stop::Memory));
        // a plugin on any other hook has the cap it always had
        assert_eq!(run(&grows(fits), &[Hook::Position]), Err(Stop::Memory));
    }

    #[test]
    fn a_long_call_is_held_to_its_fuel_by_the_fill_s_area() {
        // about 5 a turn, so past what a small fill may spend, and well inside what a large one may
        let body = "(local $i i32) (local.set $i (i32.const 6000000))
            (loop (local.set $i (i32.sub (local.get $i) (i32.const 1))) (br_if 0 (local.get $i)))
            (i32.const 0)";
        let bytes = fill_module(FILL_AT, body, "");
        let mut rows = vec![!0_u64; 1];
        assert_eq!(started(&bytes).fill(&mut rows, 64, 1, (0, 0)), Err(Stop::TooSlow));
        let mut rows = vec![!0_u64; 1024 * 16];
        assert_eq!(started(&bytes).fill(&mut rows, 1024, 1024, (0, 0)), Ok(false));
    }

    #[test]
    fn a_loop_for_ever_runs_out_of_fuel() {
        let mut running = started(&position_module("", "(loop (br 0)) (i64.const 0)"));
        assert_eq!(running.position(1.0, 2.0), Err(Stop::TooSlow));
    }

    #[test]
    fn a_trap_or_a_stack_run_out_is_a_crash() {
        let mut running = started(&position_module("", "(unreachable)"));
        assert_eq!(running.position(1.0, 2.0), Err(Stop::Crashed));
        let mut running = started(&position_module("(memory 1)", "(i64.load (i32.const 70000))"));
        assert_eq!(running.position(1.0, 2.0), Err(Stop::Crashed));
        let mut running = started(&position_module(r#"(func $down (result i64) (call $down))"#, "(call $down)"));
        assert_eq!(running.position(1.0, 2.0), Err(Stop::Crashed));
    }

    #[test]
    fn a_position_that_makes_no_sense_is_refused() {
        let packed = |x: f32, y: f32| x.to_bits() as u64 | (y.to_bits() as u64) << 32;
        let says =
            |bits: u64| started(&position_module("", &format!("(i64.const {})", bits as i64))).position(1.0, 2.0);
        assert_eq!(says(packed(4.0, 5.0)), Ok((4.0, 5.0)));
        for bad in [
            packed(f32::NAN, 2.0),
            packed(1.0, f32::NAN),
            packed(f32::INFINITY, 2.0),
            packed(1.0, f32::NEG_INFINITY),
            packed(1e30, 2.0),
            packed(1.0, -1e30),
            packed(MOST_MOVE + 2.0, 2.0),
        ] {
            assert_eq!(says(bad), Err(Stop::Nonsense), "{bad:#x}");
        }
    }

    #[test]
    fn asking_for_too_much_memory_stops_it() {
        // as it starts, by what it declares
        let pages = MOST_MEMORY / 65536 + 1;
        let declared = position_module(&format!("(memory {pages})"), "(i64.const 0)");
        assert!(can_run(&declared, &[Hook::Position], false), "it can't be seen without running it");
        assert_eq!(Running::start(&declared, &[]).err(), Some(Stop::Memory));
        // and as it runs, by what it grows to
        let grows = position_module("(memory 1)", &format!("(drop (memory.grow (i32.const {pages}))) (i64.const 0)"));
        assert_eq!(started(&grows).position(1.0, 2.0), Err(Stop::Memory));
        // what is inside the cap is no trouble
        let fits = position_module("(memory 1)", "(drop (memory.grow (i32.const 16))) (i64.const 0)");
        assert_eq!(started(&fits).position(0.0, 0.0), Ok((0.0, 0.0)));
    }

    #[test]
    fn a_start_that_never_ends_stops_it_too() {
        let bytes = position_module("(func $start (loop (br 0))) (start $start)", "(i64.const 0)");
        assert_eq!(Running::start(&bytes, &[]).err(), Some(Stop::TooSlow));
    }

    #[test]
    fn fuel_is_given_afresh_for_each_sample() {
        // a little over half a sample's worth: it would run out on the second if it didn't refill
        let body = "(local $i i32) (local.set $i (i32.const 10000))
            (loop (local.set $i (i32.sub (local.get $i) (i32.const 1))) (br_if 0 (local.get $i)))
            (i64.const 0)";
        let mut running = started(&position_module("", body));
        for _ in 0..4 {
            assert_eq!(running.position(0.0, 0.0), Ok((0.0, 0.0)));
        }
    }
}
