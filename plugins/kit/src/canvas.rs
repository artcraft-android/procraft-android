//! A kind of artwork of the plugin's own (its manifest's `kinds`): the plugin keeps the artwork,
//! the app hands it touches, asks it what to draw and keeps what it saves as each step. See
//! `docs/plugins.md`. [`canvas!`](crate::canvas!) exports the calls; what to draw is a [`DrawList`].
//!
//! ```ignore
//! kit::canvas!(MyArtwork);
//! impl kit::canvas::Artwork for MyArtwork { ... }
//! ```

use std::sync::Mutex;

/// A touch's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Moved,
    Up,
    /// Called off: a second finger made it a gesture.
    Off,
}

impl Phase {
    pub fn of(n: i32) -> Phase {
        match n {
            0 => Phase::Down,
            1 => Phase::Moved,
            2 => Phase::Up,
            _ => Phase::Off,
        }
    }
}

/// What to draw: paths, each filled or stroked or both, in canvas pixels.
#[derive(Default)]
pub struct DrawList(Vec<u32>);

impl DrawList {
    /// A path starts: filled with `fill` and stroked with `stroke` (0xRRGGBBAA, straight; alpha 0
    /// for none) `width` wide, its ends and corners round if `round`.
    pub fn path(&mut self, fill: u32, stroke: u32, width: f32, round: bool) -> &mut Self {
        self.0.extend([1, fill, stroke, width.to_bits(), u32::from(round)]);
        self
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.0.extend([2, x.to_bits(), y.to_bits()]);
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.0.extend([3, x.to_bits(), y.to_bits()]);
        self
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> &mut Self {
        self.0.extend([4, x1.to_bits(), y1.to_bits(), x2.to_bits(), y2.to_bits(), x.to_bits(), y.to_bits()]);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.0.push(5);
        self
    }

    /// The list as the app reads it.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

/// The tool picked as a stroke starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paint,
    Erase,
    Smudge,
}

/// What a stroke starts with: the tool, the colour (0xRRGGBBAA), the brush's width (canvas pixels)
/// and its opacity (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    pub colour: u32,
    pub size: f32,
    pub opacity: f32,
}

/// A kind of artwork: what the plugin keeps and how it takes touches, draws, saves and loads.
pub trait Artwork: Sized + Send {
    /// A new one, `width` × `height` canvas pixels.
    fn new(width: u32, height: u32) -> Self;
    /// The brush the next stroke starts with (told before its first touch).
    fn brush(&mut self, _brush: Brush) {}
    /// A touch at (`x`, `y`), canvas pixels: whether what it draws changed.
    fn touch(&mut self, phase: Phase, x: f32, y: f32, pressure: f32) -> bool;
    fn draw(&self, list: &mut DrawList);
    fn save(&self) -> Vec<u8>;
    /// The one `data` (what it saved) holds; none if it doesn't hold up.
    fn load(width: u32, height: u32, data: &[u8]) -> Option<Self>;
}

/// The artwork open (one plugin is one module, so one at a time), its size, and what the app reads
/// from or writes into it.
pub struct Held<A> {
    pub artwork: Mutex<Option<(A, u32, u32)>>,
    pub out: Mutex<Vec<u8>>,
    pub input: Mutex<Vec<u8>>,
}

impl<A> Held<A> {
    pub const fn new() -> Self {
        Held { artwork: Mutex::new(None), out: Mutex::new(Vec::new()), input: Mutex::new(Vec::new()) }
    }
}

impl<A> Default for Held<A> {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<A: Artwork> Held<A> {
    pub fn new_artwork(&self, width: i32, height: i32) -> i32 {
        let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else { return 1 };
        *lock(&self.artwork) = Some((A::new(w, h), w, h));
        0
    }

    pub fn tool(&self, tool: i32, colour: i32, size: f32, opacity: f32) {
        let tool = match tool {
            1 => Tool::Erase,
            2 => Tool::Smudge,
            _ => Tool::Paint,
        };
        if let Some((artwork, _, _)) = lock(&self.artwork).as_mut() {
            artwork.brush(Brush { tool, colour: colour as u32, size, opacity });
        }
    }

    pub fn touch(&self, phase: i32, x: f32, y: f32, pressure: f32) -> i32 {
        let mut held = lock(&self.artwork);
        let Some((artwork, _, _)) = held.as_mut() else { return 0 };
        i32::from(artwork.touch(Phase::of(phase), x, y, pressure))
    }

    pub fn draw(&self) -> i32 {
        let mut list = DrawList::default();
        if let Some((artwork, _, _)) = lock(&self.artwork).as_ref() {
            artwork.draw(&mut list);
        }
        self.put(list.bytes())
    }

    pub fn save(&self) -> i32 {
        let data = lock(&self.artwork).as_ref().map(|(artwork, _, _)| artwork.save()).unwrap_or_default();
        self.put(data)
    }

    fn put(&self, bytes: Vec<u8>) -> i32 {
        let len = bytes.len() as i32;
        *lock(&self.out) = bytes;
        len
    }

    pub fn out_at(&self) -> i32 {
        lock(&self.out).as_ptr() as usize as i32
    }

    pub fn input_at(&self, len: i32) -> i32 {
        let mut input = lock(&self.input);
        *input = vec![0; len.max(0) as usize];
        input.as_mut_ptr() as usize as i32
    }

    pub fn load(&self) -> i32 {
        let mut held = lock(&self.artwork);
        let Some((_, w, h)) = held.as_ref().map(|(_, w, h)| ((), *w, *h)) else { return 1 };
        let data = std::mem::take(&mut *lock(&self.input));
        match A::load(w, h, &data) {
            Some(artwork) => {
                *held = Some((artwork, w, h));
                0
            }
            None => 1,
        }
    }
}

/// Exports the calls a plugin's own kind of artwork is reached through, for `$artwork` (a type
/// that is a [`canvas::Artwork`](crate::canvas::Artwork)).
#[macro_export]
macro_rules! canvas {
    ($artwork:ty) => {
        static HELD: $crate::canvas::Held<$artwork> = $crate::canvas::Held::new();

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_new(width: i32, height: i32) -> i32 {
            HELD.new_artwork(width, height)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_tool(tool: i32, colour: i32, size: f32, opacity: f32) {
            HELD.tool(tool, colour, size, opacity)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_touch(phase: i32, x: f32, y: f32, pressure: f32) -> i32 {
            HELD.touch(phase, x, y, pressure)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_draw() -> i32 {
            HELD.draw()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_out() -> i32 {
            HELD.out_at()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_save() -> i32 {
            HELD.save()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_in(len: i32) -> i32 {
            HELD.input_at(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_canvas_load() -> i32 {
            HELD.load()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draw_list_is_written_as_the_app_reads_it() {
        let mut list = DrawList::default();
        list.path(0xFF0000FF, 0, 0.0, false).move_to(1.0, 2.0).line_to(3.0, 4.0).close();
        let bytes = list.bytes();
        assert_eq!(bytes.len(), (5 + 3 + 3 + 1) * 4);
        assert_eq!(&bytes[..4], &1u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &2u32.to_le_bytes());
        assert_eq!(&bytes[24..28], &1.0f32.to_bits().to_le_bytes());
    }

    struct Dot(f32, f32);

    impl Artwork for Dot {
        fn new(_: u32, _: u32) -> Self {
            Dot(0.0, 0.0)
        }
        fn touch(&mut self, _: Phase, x: f32, y: f32, _: f32) -> bool {
            *self = Dot(x, y);
            true
        }
        fn draw(&self, list: &mut DrawList) {
            list.path(0xFF, 0, 0.0, false).move_to(self.0, self.1);
        }
        fn save(&self) -> Vec<u8> {
            [self.0.to_le_bytes(), self.1.to_le_bytes()].concat()
        }
        fn load(_: u32, _: u32, data: &[u8]) -> Option<Self> {
            let n = |i: usize| Some(f32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?));
            Some(Dot(n(0)?, n(4)?))
        }
    }

    #[test]
    fn an_artwork_held_takes_touches_saves_and_loads() {
        let held: Held<Dot> = Held::new();
        assert_eq!(held.new_artwork(10, 10), 0);
        assert_eq!(held.touch(0, 3.0, 4.0, 1.0), 1);
        assert_eq!(held.save(), 8);
        let saved = held.out.lock().unwrap().clone();
        held.touch(0, 7.0, 7.0, 1.0);
        held.input_at(8);
        *held.input.lock().unwrap() = saved;
        assert_eq!(held.load(), 0);
        let (dot, _, _) = held.artwork.lock().unwrap().take().unwrap();
        assert_eq!((dot.0, dot.1), (3.0, 4.0));
    }
}
