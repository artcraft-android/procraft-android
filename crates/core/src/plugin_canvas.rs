//! A plugin's own canvas (interface 2): an artwork of a kind a plugin makes, a vector drawing say.
//! The plugin keeps the artwork itself, in its own format; the app hands it the touches, asks it
//! what to draw, keeps what it saves as each step and hands an older one back to undo. Its manifest
//! lists the kinds (`"kinds": [{ "key": "vector", "label": "Vector artwork" }]`), and its module
//! exports:
//!
//! - `procraft_canvas_new(width: i32, height: i32) -> i32`: a new artwork that size, canvas pixels
//!   (0 if it was made).
//! - `procraft_canvas_touch(phase: i32, x: f32, y: f32, pressure: f32) -> i32`: a touch (0 down, 1
//!   moved, 2 up, 3 called off), canvas pixels; not 0 if what it draws changed.
//! - `procraft_canvas_draw() -> i32`: what it draws now, as a draw list (below) at
//!   `procraft_canvas_out()`; the list's length in bytes.
//! - `procraft_canvas_save() -> i32`: its artwork as data of its own at `procraft_canvas_out()`;
//!   the data's length.
//! - `procraft_canvas_in(len: i32) -> i32` gives where `len` bytes go, and
//!   `procraft_canvas_load() -> i32` takes them as its artwork (0 if it could).
//!
//! - `procraft_canvas_tool(tool: i32, colour: i32, size: f32, opacity: f32)`, if it exports it: as
//!   each stroke starts, the tool picked (0 paint, 1 erase, 2 smudge), the colour (0xRRGGBBAA), the
//!   brush's width (canvas pixels) and its opacity (0 to 1).
//!
//! The draw list is little-endian 32-bit words: `1 fill stroke width flags` starts a path (fill
//! and stroke as 0xRRGGBBAA, straight; the stroke's width a float; flags 1 round caps and joins, 2
//! the even-odd rule), then `2 x y` moves, `3 x y` draws a line, `4 x1 y1 x2 y2 x y` a cubic curve
//! and `5` closes; coordinates are floats, canvas pixels. A path ends where the next starts.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::plugin_run::{Running, Stop};

/// The most words a draw list may have, and the most bytes an artwork's data may be.
const MOST_WORDS: usize = 4 << 20;
pub(crate) const MOST_DATA: usize = 64 << 20;
/// The largest canvas a plugin's artwork may be, pixels a side.
const MOST_SIDE: u32 = 16_384;

/// One path of a draw list: how it is filled and stroked, and its parts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawPath {
    pub(crate) fill: u32,
    pub(crate) stroke: u32,
    pub(crate) width: f32,
    pub(crate) flags: u32,
    pub(crate) parts: Vec<Part>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Part {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Close,
}

/// The paths `bytes` hold as a draw list, if it holds up: whole words, each path started before
/// its parts, every number a number. None for one that doesn't.
pub(crate) fn read_draw_list(bytes: &[u8]) -> Option<Vec<DrawPath>> {
    if !bytes.len().is_multiple_of(4) || bytes.len() / 4 > MOST_WORDS {
        return None;
    }
    let words: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|w| u32::from_le_bytes(*w)).collect();
    let mut paths: Vec<DrawPath> = Vec::new();
    let mut at = 0;
    let number = |i: usize| words.get(i).map(|w| f32::from_bits(*w)).filter(|n| n.is_finite() && n.abs() <= 1e7);
    while let Some(&word) = words.get(at) {
        let taken = match word {
            1 => {
                let (fill, stroke) = (*words.get(at + 1)?, *words.get(at + 2)?);
                let width = number(at + 3).filter(|w| *w >= 0.0)?;
                let flags = *words.get(at + 4)?;
                paths.push(DrawPath { fill, stroke, width, flags, parts: Vec::new() });
                5
            }
            2 | 3 => {
                let (x, y) = (number(at + 1)?, number(at + 2)?);
                let part = if word == 2 { Part::Move(x, y) } else { Part::Line(x, y) };
                paths.last_mut()?.parts.push(part);
                3
            }
            4 => {
                let mut n = [0.0; 6];
                for (i, kept) in n.iter_mut().enumerate() {
                    *kept = number(at + 1 + i)?;
                }
                paths.last_mut()?.parts.push(Part::Cubic(n));
                7
            }
            5 => {
                paths.last_mut()?.parts.push(Part::Close);
                1
            }
            _ => return None,
        };
        at += taken;
    }
    Some(paths)
}

/// The colour 0xRRGGBBAA as tiny-skia takes it.
fn colour(rgba: u32) -> tiny_skia::Color {
    let [r, g, b, a] = rgba.to_be_bytes();
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

/// `paths` drawn on a clear canvas `width` × `height`: premultiplied RGBA rows, smoothed edges.
#[cfg(test)]
pub(crate) fn rasterise(paths: &[DrawPath], width: u32, height: u32) -> Vec<u8> {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(width.max(1), height.max(1)) else {
        return vec![0; width as usize * height as usize * 4];
    };
    draw_paths(&mut pixmap, paths);
    pixmap.take()
}

/// `paths` drawn over what `pixmap` holds.
fn draw_paths(pixmap: &mut tiny_skia::Pixmap, paths: &[DrawPath]) {
    for path in paths {
        let mut pb = tiny_skia::PathBuilder::new();
        for part in &path.parts {
            match *part {
                Part::Move(x, y) => pb.move_to(x, y),
                Part::Line(x, y) => pb.line_to(x, y),
                Part::Cubic([x1, y1, x2, y2, x, y]) => pb.cubic_to(x1, y1, x2, y2, x, y),
                Part::Close => pb.close(),
            }
        }
        let Some(built) = pb.finish() else { continue };
        let paint = |rgba: u32| {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(colour(rgba));
            paint.anti_alias = true;
            paint
        };
        if path.fill & 0xFF != 0 {
            let rule = if path.flags & 2 != 0 { tiny_skia::FillRule::EvenOdd } else { tiny_skia::FillRule::Winding };
            pixmap.fill_path(&built, &paint(path.fill), rule, tiny_skia::Transform::identity(), None);
        }
        if path.stroke & 0xFF != 0 && path.width > 0.0 {
            let round = path.flags & 1 != 0;
            let stroke = tiny_skia::Stroke {
                width: path.width,
                line_cap: if round { tiny_skia::LineCap::Round } else { tiny_skia::LineCap::Butt },
                line_join: if round { tiny_skia::LineJoin::Round } else { tiny_skia::LineJoin::Miter },
                ..tiny_skia::Stroke::default()
            };
            pixmap.stroke_path(&built, &paint(path.stroke), &stroke, tiny_skia::Transform::identity(), None);
        }
    }
}

/// What changed in a picture since the one before: all of it, a rectangle (x, y, width, height,
/// pixels), or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Changed {
    Whole,
    Part(u32, u32, u32, u32),
    Nothing,
}

/// A box on the canvas: its left, top, right and bottom, canvas pixels.
type Reach = (f32, f32, f32, f32);

/// The box `paths` reach (strokes' widths and a pixel of smoothing round them).
fn reach(paths: &[DrawPath]) -> Option<Reach> {
    let mut reach: Option<Reach> = None;
    for path in paths {
        let grow = path.width / 2.0 + 2.0;
        for part in &path.parts {
            let points: &[f32] = match part {
                Part::Move(x, y) | Part::Line(x, y) => &[*x, *y],
                Part::Cubic(n) => n,
                Part::Close => &[],
            };
            for &[x, y] in points.as_chunks::<2>().0 {
                reach = Some(match reach {
                    None => (x - grow, y - grow, x + grow, y + grow),
                    Some((x0, y0, x1, y1)) => (x0.min(x - grow), y0.min(y - grow), x1.max(x + grow), y1.max(y + grow)),
                });
            }
        }
    }
    reach
}

/// A picture of a draw list kept to draw on: the list's bytes and what they drew.
struct Base {
    list: Vec<u8>,
    pixmap: tiny_skia::Pixmap,
}

/// `paths` as an SVG picture `width` × `height`: each path a `<path>` with its fill and stroke.
pub(crate) fn svg(paths: &[DrawPath], width: u32, height: u32) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    );
    // a colour as SVG takes it: its hex, and its opacity apart
    let paint = |rgba: u32| {
        let [r, g, b, a] = rgba.to_be_bytes();
        (format!("#{r:02x}{g:02x}{b:02x}"), f32::from(a) / 255.0)
    };
    for path in paths {
        // (a path whose first part isn't a move starts where the picture does, as it is drawn)
        let mut d =
            if matches!(path.parts.first(), Some(Part::Move(..)) | None) { String::new() } else { "M0 0".to_owned() };
        for part in &path.parts {
            let _ = match *part {
                Part::Move(x, y) => write!(d, "M{x} {y}"),
                Part::Line(x, y) => write!(d, "L{x} {y}"),
                Part::Cubic([x1, y1, x2, y2, x, y]) => write!(d, "C{x1} {y1} {x2} {y2} {x} {y}"),
                Part::Close => write!(d, "Z"),
            };
        }
        if d.is_empty() {
            continue;
        }
        let _ = write!(out, "<path d=\"{d}\"");
        if path.fill & 0xFF != 0 {
            let (colour, opacity) = paint(path.fill);
            let rule = if path.flags & 2 != 0 { " fill-rule=\"evenodd\"" } else { "" };
            let _ = write!(out, " fill=\"{colour}\" fill-opacity=\"{opacity:.3}\"{rule}");
        } else {
            out.push_str(" fill=\"none\"");
        }
        if path.stroke & 0xFF != 0 && path.width > 0.0 {
            let (colour, opacity) = paint(path.stroke);
            let ends = if path.flags & 1 != 0 { "round" } else { "butt" };
            let joins = if path.flags & 1 != 0 { "round" } else { "miter" };
            let _ = write!(
                out,
                " stroke=\"{colour}\" stroke-opacity=\"{opacity:.3}\" stroke-width=\"{}\" stroke-linecap=\"{ends}\" stroke-linejoin=\"{joins}\"",
                path.width
            );
        }
        out.push_str("/>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// A plugin's kind of artwork, as the gallery lists it: `key` is `<id>.<key>`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginKind {
    pub key: String,
    pub label: String,
}

/// A plugin's artwork open: the copy of its code that keeps it, and its size.
#[derive(uniffi::Object)]
pub struct PluginCanvas {
    pub(crate) id: String,
    width: u32,
    height: u32,
    running: Mutex<Running>,
    /// What it drew last, kept until a touch changes it.
    drawn: Mutex<Option<Arc<Vec<u8>>>>,
    /// A whole picture to draw on (what it drew when a stroke last ended): a list that begins as
    /// that one did is drawn as only what it adds, over it.
    base: Mutex<Option<Base>>,
    /// A stroke ended: the next picture is the base the next ones draw on.
    settle: std::sync::atomic::AtomicBool,
    /// What the last picture changed, the box of what was drawn over the base for it (what the next
    /// one changes reaches that far too), and whether it was the base and what was drawn over it
    /// (else what shows isn't, and the next picture changes all of it).
    changed: Mutex<(Changed, Option<Reach>, bool)>,
    /// Why it stopped, if it did: it takes no more touches.
    stopped: Mutex<Option<Stop>>,
    /// The plugins on, told when it stops: its card says why, and the canvas tells it once.
    hooks: Arc<crate::hooks::Hooks>,
}

impl std::fmt::Debug for PluginCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCanvas").field("id", &self.id).field("width", &self.width).finish_non_exhaustive()
    }
}

impl PluginCanvas {
    /// A new artwork `width` × `height` in `running`, or the one `data` holds; none if the plugin
    /// can't make it.
    /// Err(Some(stop)) if the plugin stopped making it (it goes off then), Err(None) if it didn't
    /// make it.
    pub(crate) fn open(
        id: String,
        hooks: Arc<crate::hooks::Hooks>,
        mut running: Running,
        width: u32,
        height: u32,
        data: Option<&[u8]>,
    ) -> Result<PluginCanvas, Option<Stop>> {
        if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE {
            return Err(None);
        }
        let made = running.canvas_new(width, height).map_err(Some)?;
        let loaded = match data {
            Some(data) => made && running.canvas_load(data).map_err(Some)?,
            None => made,
        };
        if !loaded {
            return Err(None);
        }
        Ok(PluginCanvas {
            id,
            width,
            height,
            running: Mutex::new(running),
            drawn: Mutex::new(None),
            base: Mutex::new(None),
            settle: std::sync::atomic::AtomicBool::new(true),
            changed: Mutex::new((Changed::Whole, None, false)),
            stopped: Mutex::new(None),
            hooks,
        })
    }

    fn running(&self) -> MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `ran`, or none and the plugin noted as stopped: the first time, it goes off, and says why.
    fn kept<T>(&self, ran: Result<T, Stop>) -> Option<T> {
        ran.map_err(|stop| {
            let first = self.stopped.lock().unwrap_or_else(|e| e.into_inner()).replace(stop).is_none();
            if first {
                self.hooks.stopped(&self.id, stop);
            }
        })
        .ok()
    }

    /// `list` (its `paths` read) drawn: over the base, as only what it adds, when it begins as the
    /// base's did; else whole. Kept as the base when a stroke has just ended, or there is none.
    fn drawn_over_base(&self, list: &[u8], paths: &[DrawPath]) -> Vec<u8> {
        let mut base = self.base.lock().unwrap_or_else(|e| e.into_inner());
        let over = base.as_ref().and_then(|b| {
            let added = list.strip_prefix(b.list.as_slice()).filter(|_| !b.list.is_empty())?;
            Some((b.pixmap.clone(), read_draw_list(added)?))
        });
        let mut changed = self.changed.lock().unwrap_or_else(|e| e.into_inner());
        let settle = self.settle.swap(false, std::sync::atomic::Ordering::Relaxed) || base.is_none();
        let pixmap = match over {
            Some((mut pixmap, added)) => {
                draw_paths(&mut pixmap, &added);
                // what was drawn over the base now, and what was before it (gone now); all of it if
                // what shows wasn't drawn from this base
                let now = reach(&added);
                let box_ = match (now, changed.1) {
                    (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
                    (a, b) => a.or(b),
                };
                let part = if changed.2 { self.part_of(box_) } else { Changed::Whole };
                *changed = (part, now, true);
                pixmap
            }
            None => {
                // drawn whole: from the base only if it becomes the base
                *changed = (Changed::Whole, None, settle);
                let Some(mut pixmap) = tiny_skia::Pixmap::new(self.width, self.height) else {
                    return vec![0; self.width as usize * self.height as usize * 4];
                };
                draw_paths(&mut pixmap, paths);
                pixmap
            }
        };
        drop(changed);
        if settle {
            *base = Some(Base { list: list.to_vec(), pixmap: pixmap.clone() });
        }
        pixmap.take()
    }

    /// `box_` (canvas pixels) as whole pixels on the canvas: nothing if there is none or it is off
    /// the canvas.
    fn part_of(&self, box_: Option<Reach>) -> Changed {
        let Some((x0, y0, x1, y1)) = box_ else { return Changed::Nothing };
        let clamp = |v: f32, most: u32| v.floor().clamp(0.0, most as f32) as u32;
        let (left, top) = (clamp(x0, self.width), clamp(y0, self.height));
        let (right, bottom) = (clamp(x1.ceil(), self.width), clamp(y1.ceil(), self.height));
        if right <= left || bottom <= top {
            Changed::Nothing
        } else {
            Changed::Part(left, top, right - left, bottom - top)
        }
    }

    /// What the last picture changed from the one before.
    pub(crate) fn changed(&self) -> Changed {
        self.changed.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    /// Why it stopped, if it did.
    pub(crate) fn stop(&self) -> Option<Stop> {
        *self.stopped.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl PluginCanvas {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl PluginCanvas {
    /// Control `index` of its plugin changed to `value`: told, and what it draws drawn again.
    pub(crate) fn control(&self, index: usize, value: f32) {
        if self.stop().is_none() {
            let told = self.running().control(index, value);
            self.kept(told);
            *self.drawn.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.settle.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The tool a stroke starts with, told to the plugin if it asks (see the module's notes).
    pub(crate) fn tool(&self, tool: u32, rgba: u32, size: f32, opacity: f32) {
        if self.stop().is_none() {
            let told = self.running().canvas_tool(tool, rgba, size, opacity);
            self.kept(told);
        }
    }
}

#[uniffi::export]
impl PluginCanvas {
    /// A touch (`phase` 0 down, 1 moved, 2 up, 3 called off) at (`x`, `y`), canvas pixels: whether
    /// what it draws changed. False once it stopped.
    pub fn touch(&self, phase: u32, x: f32, y: f32, pressure: f32) -> bool {
        if self.stop().is_some() || phase > 3 || !x.is_finite() || !y.is_finite() {
            return false;
        }
        let changed = self.kept(self.running().canvas_touch(phase, x, y, pressure.clamp(0.0, 1.0)));
        if changed == Some(true) {
            *self.drawn.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        // as a stroke ends, what it draws then is the base the next ones draw on
        if phase >= 2 {
            self.settle.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        changed.unwrap_or(false)
    }

    /// What it draws now: premultiplied RGBA rows, `width` × `height`. Clear once it stopped, or
    /// if what it drew doesn't hold up (it stops).
    pub fn picture(&self) -> Vec<u8> {
        if let Some(drawn) = self.drawn.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            self.changed.lock().unwrap_or_else(|e| e.into_inner()).0 = Changed::Nothing;
            return drawn.to_vec();
        }
        let clear = || vec![0; self.width as usize * self.height as usize * 4];
        if self.stop().is_some() {
            return clear();
        }
        let list = self.kept(self.running().canvas_draw());
        let Some(list) = list else { return clear() };
        let Some(paths) = read_draw_list(&list) else {
            self.kept::<()>(Err(Stop::Nonsense));
            return clear();
        };
        let picture = Arc::new(self.drawn_over_base(&list, &paths));
        *self.drawn.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&picture));
        picture.to_vec()
    }

    /// What it draws now as an SVG picture, its shapes kept as they are; none once it stopped or if
    /// what it drew doesn't hold up.
    pub fn svg(&self) -> Option<String> {
        if self.stop().is_some() {
            return None;
        }
        let list = self.kept(self.running().canvas_draw())?;
        Some(svg(&read_draw_list(&list)?, self.width, self.height))
    }

    /// Its artwork as its own data, to keep (a step undo goes back to, or the file); none once it
    /// stopped.
    pub fn save(&self) -> Option<Vec<u8>> {
        if self.stop().is_some() {
            return None;
        }
        self.kept(self.running().canvas_save())
    }

    /// `data` (what it saved) taken back as its artwork: undo and redo. False if it couldn't.
    pub fn load(&self, data: Vec<u8>) -> bool {
        if self.stop().is_some() || data.len() > MOST_DATA {
            return false;
        }
        let loaded = self.kept(self.running().canvas_load(&data)).unwrap_or(false);
        *self.drawn.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // what it draws now starts afresh
        self.settle.store(true, std::sync::atomic::Ordering::Relaxed);
        loaded
    }

    /// Why it stopped, in words for its card, or none while it runs.
    pub fn problem(&self) -> Option<String> {
        self.stop().map(|stop| stop.text().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[u32]) -> Vec<u8> {
        list.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn f(n: f32) -> u32 {
        n.to_bits()
    }

    #[test]
    fn a_draw_list_is_read_and_drawn() {
        // a red square filled, and a blue line stroked 4 wide with round ends
        let list = words(&[
            1,
            0xFF0000FF,
            0,
            f(0.0),
            0, //
            2,
            f(2.0),
            f(2.0),
            3,
            f(8.0),
            f(2.0),
            3,
            f(8.0),
            f(8.0),
            3,
            f(2.0),
            f(8.0),
            5, //
            1,
            0,
            0x0000FFFF,
            f(4.0),
            1, //
            2,
            f(12.0),
            f(15.0),
            4,
            f(14.0),
            f(15.0),
            f(16.0),
            f(15.0),
            f(18.0),
            f(15.0),
        ]);
        let paths = read_draw_list(&list).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].parts.len(), 5);
        let rgba = rasterise(&paths, 20, 20);
        let at = |x: usize, y: usize| rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].to_vec();
        assert_eq!(at(5, 5), [255, 0, 0, 255], "inside the square");
        assert_eq!(at(0, 0)[3], 0, "outside");
        assert_eq!(at(15, 15), [0, 0, 255, 255], "on the line");
        assert_eq!(at(15, 18)[3], 0, "past its width");
    }

    #[test]
    fn a_draw_list_is_written_as_svg() {
        let list = words(&[
            1,
            0xFF000080,
            0,
            f(0.0),
            2,
            2,
            f(2.0),
            f(2.0),
            3,
            f(8.0),
            f(2.0),
            5, //
            1,
            0,
            0x0000FFFF,
            f(4.0),
            1,
            2,
            f(1.0),
            f(1.0),
            4,
            f(2.0),
            f(2.0),
            f(3.0),
            f(3.0),
            f(4.0),
            f(4.0),
        ]);
        let text = svg(&read_draw_list(&list).unwrap(), 20, 10);
        assert!(text.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\""));
        assert!(
            text.contains("<path d=\"M2 2L8 2Z\" fill=\"#ff0000\" fill-opacity=\"0.502\" fill-rule=\"evenodd\"/>"),
            "{text}"
        );
        assert!(text.contains("d=\"M1 1C2 2 3 3 4 4\" fill=\"none\" stroke=\"#0000ff\""), "{text}");
        assert!(text.contains("stroke-linecap=\"round\""));
        assert!(text.ends_with("</svg>\n"));
    }

    #[test]
    fn a_draw_list_that_doesn_t_hold_up_is_none() {
        assert!(read_draw_list(&words(&[2, f(1.0), f(1.0)])).is_none(), "a part before a path");
        assert!(read_draw_list(&words(&[1, 0, 0, f(1.0)])).is_none(), "a path cut short");
        assert!(read_draw_list(&words(&[1, 0, 0, f(f32::NAN), 0])).is_none(), "a width that is no number");
        assert!(read_draw_list(&words(&[9])).is_none(), "a word that is nothing");
        assert!(read_draw_list(&[1, 0, 0]).is_none(), "not whole words");
        assert_eq!(read_draw_list(&[]), Some(Vec::new()), "nothing drawn");
    }
}
