//! A patch's code: a WebAssembly module, interpreted under caps on its time and memory. It gets no
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
//! - `procraft_control(index: i32, value: f32)`: a control's value (a switch is 0 or 1), by its
//!   place in the manifest: each as the patch starts, then each as it changes. Needed if it has any.

use std::panic::{AssertUnwindSafe, catch_unwind};

use wasmi::errors::ErrorKind;
use wasmi::{
    Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TrapCode,
    TypedFunc, ValType,
};

/// The module's name in a patch's folder.
pub(crate) const MODULE: &str = "patch.wasm";
/// The most a module may be, and may ask for as it runs, in bytes.
pub(crate) const MOST_MODULE: usize = 1 << 20;
const MOST_MEMORY: usize = 16 << 20;
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
const MEMORY: &str = "memory";
const CONTROL: &str = "procraft_control";

/// What a patch may step into, by the name a manifest gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hook {
    Position,
    PickLayer,
    InterfaceScale,
    PasteMask,
}

impl Hook {
    pub(crate) fn named(name: &str) -> Option<Hook> {
        match name {
            "position" => Some(Hook::Position),
            "pick_layer" => Some(Hook::PickLayer),
            "interface_scale" => Some(Hook::InterfaceScale),
            "paste_mask" => Some(Hook::PasteMask),
            _ => None,
        }
    }

    /// What it lets a patch do, for the screen that asks whether to import one.
    pub(crate) fn sentence(self) -> &'static str {
        match self {
            Hook::Position => "This patch can change where your strokes land.",
            Hook::PickLayer => "This patch can make the eyedropper read the current layer alone.",
            Hook::InterfaceScale => "This patch can make the interface bigger or smaller.",
            Hook::PasteMask => "This patch can put what you paste into the current mask.",
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
            Hook::InterfaceScale => exports(module, INTERFACE_SCALE, &[ValType::F32, ValType::F32], &[ValType::F32]),
            Hook::PasteMask => {
                exports(module, PASTE_MASK, &[], &[ValType::I32])
                    && exports(module, ROW_BUFFER, &[ValType::I32], &[ValType::I32])
                    && exports(module, PASTE_ROW, &[ValType::I32, ValType::I32], &[])
                    && module.exports().any(|export| export.name() == MEMORY && export.ty().memory().is_some())
            }
        }
    }
}

/// Why a patch stopped, or never ran. Each stays on its card, in plain words.
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

/// `run`, a panic in the interpreter counting as a crash of the patch: a file must not take the
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

/// Whether `bytes` is a module a patch on `hooks` can be: valid, wanting nothing from outside, and
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

/// A patch's module, instantiated, with what it has of ours to call.
pub(crate) struct Running {
    store: Store<StoreLimits>,
    position: Option<TypedFunc<(f32, f32), i64>>,
    pick_layer: Option<TypedFunc<(), i32>>,
    interface_scale: Option<TypedFunc<(f32, f32), f32>>,
    paste_mask: Option<TypedFunc<(), i32>>,
    row_buffer: Option<TypedFunc<i32, i32>>,
    paste_row: Option<TypedFunc<(i32, i32), ()>>,
    memory: Option<Memory>,
    control: Option<TypedFunc<(i32, f32), ()>>,
}

impl Running {
    /// `bytes` started, under its caps, and told `controls`.
    pub(crate) fn start(bytes: &[u8], controls: &[f32]) -> Result<Running, Stop> {
        let mut running = guarded(|| {
            let engine = engine();
            let module = Module::new(&engine, bytes).map_err(|_| Stop::Damaged)?;
            let limits = StoreLimitsBuilder::new()
                .memory_size(MOST_MEMORY)
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

    /// Moves a sample by its two controls, as a patch of real use does.
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
    fn a_patch_moves_a_sample_by_what_its_controls_say() {
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
