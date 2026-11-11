//! The doors that hand pictures, sounds, text and files back and forth: each a macro exporting
//! what its hook asks for, the memory taken care of.

use std::sync::{Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A place in the module's memory the app writes into or reads out of, kept until the next.
pub struct Place(Mutex<Vec<u8>>);

impl Place {
    pub const fn new() -> Self {
        Place(Mutex::new(Vec::new()))
    }

    /// Room for `len` bytes, for the app to write into: where it is.
    pub fn make(&self, len: i32) -> i32 {
        let mut bytes = lock(&self.0);
        *bytes = vec![0; len.max(0) as usize];
        bytes.as_mut_ptr() as usize as i32
    }

    /// `bytes` kept for the app to read: where they are.
    pub fn put(&self, bytes: Vec<u8>) -> i32 {
        let mut kept = lock(&self.0);
        *kept = bytes;
        kept.as_mut_ptr() as usize as i32
    }

    /// What is there, let go.
    pub fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *lock(&self.0))
    }

    /// What is there, looked at.
    pub fn with<R>(&self, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        f(&mut lock(&self.0))
    }
}

impl Default for Place {
    fn default() -> Self {
        Place::new()
    }
}

/// Where each door keeps what it hands over (one plugin is one module, so one of each).
pub static SOUND: Place = Place::new();
pub static TEXT: Place = Place::new();
pub static FILE: Place = Place::new();
pub static CURVE: Place = Place::new();

/// The stroke as the sound hook tells it: its speed (window heights a second), its pressure (a
/// finger as pressed all the way), how long since its last input (none: no stroke open), whether
/// the tool erases, where the pen hovers (window pixels), and the window's height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stroke {
    pub speed: f32,
    pub pressure: f32,
    pub since_ms: Option<f32>,
    pub erasing: bool,
    pub hover: Option<(f32, f32)>,
    pub height: f32,
}

impl Stroke {
    /// The stroke from the numbers the hook hands over.
    pub fn told(speed: f32, pressure: f32, since_ms: f32, state: i32, hover_x: f32, hover_y: f32, height: f32) -> Self {
        let open = state & 1 != 0 && since_ms >= 0.0 && since_ms.is_finite();
        Stroke {
            speed: if speed.is_finite() { speed.max(0.0) } else { 0.0 },
            pressure: if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 1.0 },
            since_ms: open.then_some(since_ms),
            erasing: state & 4 != 0,
            hover: (state & 2 != 0).then_some((hover_x, hover_y)),
            height: height.max(1.0),
        }
    }
}

/// `samples` kept for the app to play: where they are.
pub fn put_samples(samples: Vec<f32>) -> i32 {
    SOUND.put(samples.iter().flat_map(|s| s.to_le_bytes()).collect())
}

/// Exports the sound hook: `$make` (a `fn(kit::Stroke, frames: usize, rate: f32) -> Vec<f32>`)
/// makes the next `frames` samples (-1 to 1) at `rate` a second.
#[macro_export]
macro_rules! stroke_sound {
    ($make:expr) => {
        #[unsafe(no_mangle)]
        #[allow(clippy::too_many_arguments)]
        pub extern "C" fn procraft_stroke_sound(
            frames: i32,
            rate: i32,
            speed: f32,
            pressure: f32,
            since_ms: f32,
            state: i32,
            hover_x: f32,
            hover_y: f32,
            height: f32,
        ) -> i32 {
            let make: fn($crate::Stroke, usize, f32) -> Vec<f32> = $make;
            let stroke = $crate::Stroke::told(speed, pressure, since_ms, state, hover_x, hover_y, height);
            $crate::put_samples(make(stroke, frames.clamp(0, 48_000) as usize, rate.max(1) as f32))
        }
    };
}

/// Exports the sound hook's readying: `$ready` (a `fn(rate: f32)`) makes what is made once, before
/// the first sound at `rate`.
#[macro_export]
macro_rules! sound_prepare {
    ($ready:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_sound_prepare(rate: i32) -> i32 {
            let ready: fn(f32) = $ready;
            ready(rate.max(1) as f32);
            1
        }
    };
}

/// Exports what the app tells the current brush through: `$told` (a `fn(name: &str, set: &str)`).
#[macro_export]
macro_rules! brush {
    ($told:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_text_buffer(len: i32) -> i32 {
            $crate::TEXT.make(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_brush(name_len: i32, set_len: i32) {
            let told: fn(&str, &str) = $told;
            let text = $crate::TEXT.take();
            let (name_len, set_len) = (name_len.max(0) as usize, set_len.max(0) as usize);
            let name = text.get(..name_len).map(String::from_utf8_lossy).unwrap_or_default();
            let set = text.get(name_len..name_len + set_len).map(String::from_utf8_lossy).unwrap_or_default();
            told(&name, &set);
        }
    };
}

/// Exports what the app tells the current brush's rows (its `brush_controls`) through: `$told` (a
/// `fn(index: usize, value: f32)`, the row's place and its choice's).
#[macro_export]
macro_rules! brush_control {
    ($told:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_brush_control(index: i32, value: f32) {
            let told: fn(usize, f32) = $told;
            if let (Ok(index), true) = (usize::try_from(index), value.is_finite()) {
                told(index, value);
            }
        }
    };
}

/// Exports what the app hands the files picked for `file` controls through: `$took` (a
/// `fn(index: usize, file: Option<&[u8]>)`, the control's place and the file, none when let go).
#[macro_export]
macro_rules! files {
    ($took:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_file_buffer(_index: i32, len: i32) -> i32 {
            $crate::FILE.make(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_file(index: i32, len: i32) {
            let took: fn(usize, Option<&[u8]>) = $took;
            let data = $crate::FILE.take();
            let Ok(index) = usize::try_from(index) else { return };
            took(index, if len > 0 { data.get(..len as usize) } else { None });
        }
    };
}

/// An in-between under way: its size, the two frames, the frame between, and its row.
pub struct Between {
    pub width: usize,
    pub height: usize,
    pub this: Vec<u8>,
    pub next: Vec<u8>,
    pub made: Vec<u8>,
    pub row: Vec<u8>,
}

pub static BETWEEN: Mutex<Option<Between>> = Mutex::new(None);

/// Starts an in-between of `width` × `height`: where a row goes.
pub fn between_begin(width: i32, height: i32) -> i32 {
    let (width, height) = (width.max(0) as usize, height.max(0) as usize);
    let size = width * height * 4;
    let mut job = lock(&BETWEEN);
    let job = job.insert(Between {
        width,
        height,
        this: vec![0; size],
        next: vec![0; size],
        made: Vec::new(),
        row: vec![0; width * 4],
    });
    job.row.as_mut_ptr() as usize as i32
}

/// Row `y` of `frame` (0 this frame, 1 the next) is in the row.
pub fn between_row(frame: i32, y: i32) {
    let mut guard = lock(&BETWEEN);
    let Some(job) = guard.as_mut() else { return };
    let (row, Ok(y)) = (job.width * 4, usize::try_from(y)) else { return };
    let pixels = if frame == 0 { &mut job.this } else { &mut job.next };
    if let Some(to) = pixels.get_mut(y * row..(y + 1) * row) {
        to.copy_from_slice(&job.row);
    }
}

/// What draws the frame between two (this, next, width, height), premultiplied RGBA8 rows.
pub type DrawBetween = fn(&[u8], &[u8], usize, usize) -> Option<Vec<u8>>;

/// Draws the frame between by `draw`: whether there is one.
pub fn between_run(draw: DrawBetween) -> i32 {
    let mut guard = lock(&BETWEEN);
    let Some(job) = guard.as_mut() else { return 0 };
    match draw(&job.this, &job.next, job.width, job.height) {
        Some(made) if made.len() == job.width * job.height * 4 => {
            job.made = made;
            1
        }
        _ => 0,
    }
}

/// Row `y` of the frame between, into the row.
pub fn between_out(y: i32) {
    let mut guard = lock(&BETWEEN);
    let Some(job) = guard.as_mut() else { return };
    let (row, Ok(y)) = (job.width * 4, usize::try_from(y)) else { return };
    if let Some(from) = job.made.get(y * row..(y + 1) * row) {
        job.row.copy_from_slice(from);
    }
}

/// Exports the in-between hook: `$draw` (a `fn(this: &[u8], next: &[u8], width: usize, height:
/// usize) -> Option<Vec<u8>>`, premultiplied RGBA8 each) draws the frame between two frames.
#[macro_export]
macro_rules! inbetween {
    ($draw:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_between_begin(width: i32, height: i32) -> i32 {
            $crate::between_begin(width, height)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_between_row(frame: i32, y: i32) {
            $crate::between_row(frame, y)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_between_run() -> i32 {
            $crate::between_run($draw)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_between_out(y: i32) {
            $crate::between_out(y)
        }
    };
}

/// QuickShape's kinds, as the curve hook names what it found first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Polyline,
    Arc,
    Curve,
    Ellipse,
    Circle,
    Triangle,
    EquilateralTriangle,
    Quadrilateral,
    Rectangle,
    Square,
}

impl ShapeKind {
    const ALL: [ShapeKind; 11] = [
        ShapeKind::Line,
        ShapeKind::Polyline,
        ShapeKind::Arc,
        ShapeKind::Curve,
        ShapeKind::Ellipse,
        ShapeKind::Circle,
        ShapeKind::Triangle,
        ShapeKind::EquilateralTriangle,
        ShapeKind::Quadrilateral,
        ShapeKind::Rectangle,
        ShapeKind::Square,
    ];

    /// The kind by its number; none for -1 (a curve asked for by name) or one not known.
    pub fn of(number: i32) -> Option<ShapeKind> {
        usize::try_from(number).ok().and_then(|i| ShapeKind::ALL.get(i)).copied()
    }
}

/// What QuickShape found first, while a stroke's points come in (none: a curve asked for).
pub static CURVE_BEST: Mutex<Option<Option<ShapeKind>>> = Mutex::new(None);

/// Exports the curve hook: `$fit` (a `fn(points: &[(f32, f32)], found: Option<kit::ShapeKind>) ->
/// Option<Vec<(f32, f32)>>`, `found` none when a curve is asked for by name) offers a curve's nodes
/// (an anchor, then two controls and an anchor for each span).
#[macro_export]
macro_rules! quickshape_curve {
    ($fit:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_curve_begin(count: i32, best: i32) -> i32 {
            *$crate::CURVE_BEST.lock().unwrap_or_else(|e| e.into_inner()) = Some($crate::ShapeKind::of(best));
            $crate::CURVE.make(count.max(0).saturating_mul(8))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_curve_run() -> i32 {
            let fit: fn(&[(f32, f32)], Option<$crate::ShapeKind>) -> Option<Vec<(f32, f32)>> = $fit;
            let best = $crate::CURVE_BEST.lock().unwrap_or_else(|e| e.into_inner()).take().flatten();
            let bytes = $crate::CURVE.take();
            let points: Vec<(f32, f32)> = bytes
                .chunks_exact(8)
                .map(|b| (f32::from_le_bytes([b[0], b[1], b[2], b[3]]), f32::from_le_bytes([b[4], b[5], b[6], b[7]])))
                .collect();
            match fit(&points, best) {
                Some(nodes) if nodes.len() >= 4 && nodes.len() % 3 == 1 => {
                    let count = nodes.len() as i32;
                    $crate::CURVE
                        .put(nodes.iter().flat_map(|(x, y)| [x.to_le_bytes(), y.to_le_bytes()]).flatten().collect());
                    count
                }
                _ => 0,
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_curve_nodes() -> i32 {
            $crate::CURVE.with(|bytes| bytes.as_mut_ptr() as usize as i32)
        }
    };
}
