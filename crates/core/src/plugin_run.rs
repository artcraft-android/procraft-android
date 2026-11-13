//! A plugin's code: a WebAssembly module, interpreted under caps on its time and memory. It gets no
//! files, network or clock: at a hook all it ever sees is the numbers the hook hands it, and all it
//! can do is hand some back. The only imports it may have are the app's functions its permissions
//! give (`plugin_commands.rs`, interface 2), which answer only while one of its actions runs:
//! `procraft_action(index: i32) -> i32`, called when its action at `index` is tapped (returns 0).
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
//! - For the hook `picker` (interface 2; the colour panel's Classic square drawn the plugin's way,
//!   its picture the plugin's `picture.wgsl`): `procraft_picker_pick(x: f32, y: f32) -> i64` gives
//!   the saturation and brightness at a point of the square (0..1, y down) and
//!   `procraft_picker_point(s: f32, v: f32) -> i64` where they sit, each pair packed as `position`'s.
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
//! - For the hook `key_shortcut` (a hardware keyboard's presses taken as others, the plugin's): a
//!   press is written as text, its modifiers then its key ("command+shift+z", "space", see
//!   [`crate::KeyCombo`]), into `procraft_key_buffer(len: i32) -> i32`; then
//!   `procraft_key_shortcut(len: i32) -> i64` says what the press is taken as, and
//!   `procraft_key_shortcut_keys(len: i32) -> i64` which presses now do what this one does by
//!   default, for the shortcuts' list. Each answers 0 for as it is, below 0 for none, else where its
//!   text is in its `memory` (low half) and how long (high half): one press, or presses with a
//!   space between. `procraft_key_shortcuts(len: i32)`, if it has one, is told the app's shortcuts
//!   as it starts (lines of the group, what it does, its presses and 1 if the app keeps its keys,
//!   tab between: [`crate::keys::shortcuts_told`]).
//! - `procraft_panel_key(len: i32) -> i32`: while its panel asks for a key press, the next one,
//!   written as above into `procraft_key_buffer`; it acts as a touch on the panel does.
//! - What a plugin keeps (interface 2, at most [`MOST_KEPT`] bytes, with its other files): after an
//!   action, a touch on its panel or a key press its panel asked for, `procraft_keep() -> i64`, if it
//!   has one, says what to keep from now on, packed as a key answer's text (below 0: nothing new).
//!   Each copy of its code is told what it keeps as it starts, and whenever another copy keeps
//!   something new: into `procraft_kept_buffer(len: i32) -> i32`, then `procraft_kept(len: i32)`.
//! - `procraft_control(index: i32, value: f32)`: a control's value (a switch is 0 or 1), by its
//!   place in the manifest: each as the plugin starts, then each as it changes. Needed if it has any.
//! - For kinds of layer of its own (interface 3, its manifest's `layer_kinds`; see
//!   `plugin_layers.rs`): `procraft_layer_in(len: i32) -> i32` says where `len` bytes go and
//!   `procraft_layer_out() -> i32` where an answer is; `procraft_layer_load(kind: i32, len: i32) ->
//!   i32` takes `len` bytes there as the data of a layer of its kind `kind` (by its place in
//!   `layer_kinds`; none for a new, empty one), 0 if it could; `procraft_layer_add(len: i32) -> i32`
//!   takes an item recorded on that layer (0: kept as drawn, 1: kept, and the layer is drawn again
//!   from its items); `procraft_layer_save() -> i32` gives the layer's data now and
//!   `procraft_layer_draw() -> i32` its items to draw, each its length at `procraft_layer_out()`.
//!   If it has them, `procraft_layer_select(len: i32) -> i32` gives the ids of the items a selection
//!   (`len` bytes) takes, and `procraft_layer_edit(len: i32) -> i32` makes the changes `len` bytes
//!   ask of its items (not 0: something changed); `procraft_layer_fill(len: i32) -> i32` takes a
//!   colour dropped on the layer (not 0: its items changed); `procraft_layer_export(len: i32) -> i32`
//!   makes a file of an artwork's layers of a kind (its length at `procraft_layer_out()`, below 0
//!   none); `procraft_layer_hit(len: i32) -> i32` says which item is at a point (4 bytes, its id, at
//!   `procraft_layer_out()`, or 0 for none).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use crate::plugin_commands::{Host, PluginPermission};
use wasmi::errors::ErrorKind;
use wasmi::{
    Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimitsBuilder, TrapCode, TypedFunc,
    TypedResumableCall, ValType, WasmParams, WasmResults,
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
const PICKER_PICK: &str = "procraft_picker_pick";
const PICKER_POINT: &str = "procraft_picker_point";
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
const ACTION: &str = "procraft_action";
/// What a plugin on the `stroke_kept` or `layer_added` hook is told through (1 a stroke kept, 2 a
/// layer added).
const EVENT: &str = "procraft_event";
const PANEL_EVENT: &str = "procraft_panel_event";
const CANVAS_NEW: &str = "procraft_canvas_new";
const CANVAS_TOUCH: &str = "procraft_canvas_touch";
const CANVAS_DRAW: &str = "procraft_canvas_draw";
const CANVAS_OUT: &str = "procraft_canvas_out";
const CANVAS_SAVE: &str = "procraft_canvas_save";
const CANVAS_IN: &str = "procraft_canvas_in";
const CANVAS_LOAD: &str = "procraft_canvas_load";
const CANVAS_TOOL: &str = "procraft_canvas_tool";
const LAYER_IN: &str = "procraft_layer_in";
const LAYER_OUT: &str = "procraft_layer_out";
const LAYER_LOAD: &str = "procraft_layer_load";
const LAYER_ADD: &str = "procraft_layer_add";
const LAYER_SAVE: &str = "procraft_layer_save";
const LAYER_DRAW: &str = "procraft_layer_draw";
const LAYER_SELECT: &str = "procraft_layer_select";
const LAYER_EDIT: &str = "procraft_layer_edit";
const LAYER_FILL: &str = "procraft_layer_fill";
const LAYER_EXPORT: &str = "procraft_layer_export";
const LAYER_HIT: &str = "procraft_layer_hit";
/// What a call on a plugin's own artwork may spend (its drawing, its saving), and how long; a touch,
/// which comes with each sample while the canvas waits, far less.
const CANVAS_FUEL: u64 = 800_000_000;
const MOST_CANVAS_TIME: Duration = Duration::from_secs(5);
const TOUCH_FUEL: u64 = 20_000_000;
const MOST_TOUCH_TIME: Duration = Duration::from_millis(250);
/// What an action may spend and how long it may take: it does the plugin's work on the artwork.
/// What a call on a plugin's layer may take: as a call on its own artwork, the steps and the time;
/// and the memory a plugin with kinds of layer may ask for (a layer's data in, out and worked on).
const LAYER_FUEL: u64 = CANVAS_FUEL;
const MOST_LAYER_TIME: Duration = MOST_CANVAS_TIME;
const MOST_LAYER_MEMORY: usize = 256 << 20;
const ACTION_FUEL: u64 = 4_000_000_000;
const MOST_ACTION_TIME: Duration = Duration::from_secs(20);
pub(crate) const KEY_BUFFER: &str = "procraft_key_buffer";
const KEY_SHORTCUT: &str = "procraft_key_shortcut";
const KEY_SHORTCUT_KEYS: &str = "procraft_key_shortcut_keys";
const KEY_SHORTCUTS: &str = "procraft_key_shortcuts";
pub(crate) const PANEL_KEY: &str = "procraft_panel_key";
const KEPT_BUFFER: &str = "procraft_kept_buffer";
const KEPT: &str = "procraft_kept";
const KEEP: &str = "procraft_keep";
/// The most bytes a plugin keeps, and of the presses a key answer writes.
pub(crate) const MOST_KEPT: usize = 64 << 10;
const MOST_KEY_TEXT: usize = 4 << 10;
/// What being told what it keeps, or the app's shortcuts, may spend, and how long it may take.
const TOLD_FUEL: u64 = 20_000_000;
const MOST_TOLD_TIME: Duration = Duration::from_secs(2);

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
    Picker,
    /// Told each time a stroke is kept, acting on the artwork as an action does.
    StrokeKept,
    /// Told each time a layer is added from the Layers panel, acting as an action does.
    LayerAdded,
    /// A hardware keyboard's presses taken as others before the app's shortcuts see them.
    KeyShortcut,
    /// Kinds of layer of its own (interface 3): not named in `hooks`, but on it whenever its
    /// manifest lists `layer_kinds`.
    Layers,
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
            "picker" => Some(Hook::Picker),
            "stroke_kept" => Some(Hook::StrokeKept),
            "layer_added" => Some(Hook::LayerAdded),
            "key_shortcut" => Some(Hook::KeyShortcut),
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
            Hook::Picker => "This plugin can draw the colour panel's Classic picker its own way.",
            Hook::StrokeKept => "This plugin can act on your artwork each time you finish a stroke.",
            Hook::LayerAdded => "This plugin can act on your artwork each time you add a layer.",
            Hook::KeyShortcut => "This plugin can change what your keyboard's keys do.",
            Hook::Layers => "This plugin can add kinds of layer whose strokes it keeps, and draw them again.",
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
            Hook::StrokeKept | Hook::LayerAdded => exports(module, EVENT, &[ValType::I32], &[ValType::I32]),
            Hook::Layers => {
                exports(module, LAYER_IN, &[ValType::I32], &[ValType::I32])
                    && exports(module, LAYER_OUT, &[], &[ValType::I32])
                    && exports(module, LAYER_LOAD, &[ValType::I32, ValType::I32], &[ValType::I32])
                    && exports(module, LAYER_ADD, &[ValType::I32], &[ValType::I32])
                    && exports(module, LAYER_SAVE, &[], &[ValType::I32])
                    && exports(module, LAYER_DRAW, &[], &[ValType::I32])
                    && has_memory(module)
            }
            Hook::KeyShortcut => {
                exports(module, KEY_BUFFER, &[ValType::I32], &[ValType::I32])
                    && exports(module, KEY_SHORTCUT, &[ValType::I32], &[ValType::I64])
                    && exports(module, KEY_SHORTCUT_KEYS, &[ValType::I32], &[ValType::I64])
                    && has_memory(module)
            }
            Hook::Picker => {
                exports(module, PICKER_PICK, &[ValType::F32, ValType::F32], &[ValType::I64])
                    && exports(module, PICKER_POINT, &[ValType::F32, ValType::F32], &[ValType::I64])
            }
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

/// Two numbers `func` answers for `asked`, packed as the position hook's are, each put in 0..1; a
/// number that isn't one stops it.
fn picker_pair(
    store: &mut Store<Host>,
    func: &TypedFunc<(f32, f32), i64>,
    asked: (f32, f32),
) -> Result<(f32, f32), Stop> {
    let packed = guarded(|| {
        store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
        func.call(&mut *store, asked).map_err(|e| stop_of(&e))
    })?;
    let bits = packed as u64;
    let (a, b) = (f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32));
    if a.is_finite() && b.is_finite() { Ok((a.clamp(0.0, 1.0), b.clamp(0.0, 1.0))) } else { Err(Stop::Nonsense) }
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
        // its imports checked before, all that can keep one from starting is the caps
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

/// [`can_act`] for a plugin with no actions that may reach nothing.
#[cfg(test)]
pub(crate) fn can_run(bytes: &[u8], hooks: &[Hook], controls: bool) -> bool {
    can_act(bytes, hooks, controls, false, &[], false)
}

/// Whether `bytes` is a module a plugin on `hooks` can be: valid, wanting from outside only the
/// app's functions its `permissions` give, and exporting what it must (`controls`: it has some;
/// `actions`: it has some). Nothing in it runs.
pub(crate) fn can_act(
    bytes: &[u8],
    hooks: &[Hook],
    controls: bool,
    actions: bool,
    permissions: &[PluginPermission],
    kinds: bool,
) -> bool {
    if bytes.len() > MOST_MODULE || !code_counts_hold(bytes) {
        return false;
    }
    let fits = guarded(|| {
        let module = Module::new(&engine(), bytes).map_err(|_| Stop::Damaged)?;
        Ok(crate::plugin_commands::imports_fit(&module, permissions)
            && hooks.iter().all(|hook| hook.fits(&module))
            && (!controls || exports(&module, CONTROL, &[ValType::I32, ValType::F32], &[]))
            && (!actions || exports(&module, ACTION, &[ValType::I32], &[ValType::I32]))
            && (!permissions.contains(&PluginPermission::Panel)
                || exports(&module, PANEL_EVENT, &[ValType::I32, ValType::F32], &[ValType::I32]))
            && (!kinds || has_canvas(&module)))
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

/// What a plugin's own artwork is reached through (`plugin_canvas.rs`).
struct CanvasCalls {
    new: TypedFunc<(i32, i32), i32>,
    touch: TypedFunc<(i32, f32, f32, f32), i32>,
    draw: TypedFunc<(), i32>,
    out: TypedFunc<(), i32>,
    save: TypedFunc<(), i32>,
    input: TypedFunc<i32, i32>,
    load: TypedFunc<(), i32>,
    /// Told the tool, colour, size and opacity as a stroke starts, if it asks to be.
    tool: Option<TypedFunc<(i32, i32, f32, f32), ()>>,
}

/// What a plugin's own kinds of layer are reached through (`plugin_layers.rs`).
struct LayerCalls {
    input: TypedFunc<i32, i32>,
    out: TypedFunc<(), i32>,
    load: TypedFunc<(i32, i32), i32>,
    add: TypedFunc<i32, i32>,
    save: TypedFunc<(), i32>,
    draw: TypedFunc<(), i32>,
    /// Which items a selection takes, and changes to items, if it has them.
    select: Option<TypedFunc<i32, i32>>,
    edit: Option<TypedFunc<i32, i32>>,
    /// A colour dropped on the layer, if it takes one.
    fill: Option<TypedFunc<i32, i32>>,
    /// A file of an artwork's layers, if it makes any.
    export: Option<TypedFunc<i32, i32>>,
    /// The item at a point, if it says.
    hit: Option<TypedFunc<i32, i32>>,
}

/// What a plugin says of an item recorded on its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerAdded {
    /// Kept: the layer's pixels, the stroke painted as it was drawn, are as its items draw it.
    Kept,
    /// Kept, and what was drawn before changed (strokes taken away, say): the layer is drawn again
    /// from its items.
    DrawAgain,
}

/// Whether `module` exports what a plugin's own artwork is reached through.
fn has_canvas(module: &Module) -> bool {
    use ValType::{F32, I32};
    exports(module, CANVAS_NEW, &[I32, I32], &[I32])
        && exports(module, CANVAS_TOUCH, &[I32, F32, F32, F32], &[I32])
        && exports(module, CANVAS_DRAW, &[], &[I32])
        && exports(module, CANVAS_OUT, &[], &[I32])
        && exports(module, CANVAS_SAVE, &[], &[I32])
        && exports(module, CANVAS_IN, &[I32], &[I32])
        && exports(module, CANVAS_LOAD, &[], &[I32])
        && has_memory(module)
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
    store: &mut Store<Host>,
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
    store: Store<Host>,
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
    picker_pick: Option<TypedFunc<(f32, f32), i64>>,
    picker_point: Option<TypedFunc<(f32, f32), i64>>,
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
    action: Option<TypedFunc<i32, i32>>,
    panel_event: Option<TypedFunc<(i32, f32), i32>>,
    event: Option<TypedFunc<i32, i32>>,
    canvas: Option<CanvasCalls>,
    layers: Option<LayerCalls>,
    key_buffer: Option<TypedFunc<i32, i32>>,
    key_shortcut: Option<TypedFunc<i32, i64>>,
    key_shortcut_keys: Option<TypedFunc<i32, i64>>,
    key_shortcuts: Option<TypedFunc<i32, ()>>,
    panel_key: Option<TypedFunc<i32, i32>>,
    kept_buffer: Option<TypedFunc<i32, i32>>,
    kept: Option<TypedFunc<i32, ()>>,
    keep: Option<TypedFunc<(), i64>>,
}

/// Where a key answer's text is and how long, or what it says without one: as it is (none), or
/// no press (an empty list).
fn key_answer(store: &Store<Host>, memory: &Memory, packed: i64) -> Result<Option<Vec<u8>>, Stop> {
    if packed == 0 {
        return Ok(None);
    }
    if packed < 0 {
        return Ok(Some(Vec::new()));
    }
    let (at, len) = (packed as u64 as u32 as usize, (packed as u64 >> 32) as usize);
    if len > MOST_KEPT {
        return Err(Stop::Nonsense);
    }
    let mut bytes = vec![0_u8; len];
    memory.read(store, at, &mut bytes).map_err(|_| Stop::Nonsense)?;
    Ok(Some(bytes))
}

/// The presses a key answer's text writes; one that writes none that hold up stops the plugin.
fn presses(text: &[u8]) -> Result<Vec<crate::KeyCombo>, Stop> {
    let text = std::str::from_utf8(text).map_err(|_| Stop::Nonsense)?;
    if text.len() > MOST_KEY_TEXT {
        return Err(Stop::Nonsense);
    }
    crate::KeyCombo::read_all(text).ok_or(Stop::Nonsense)
}

impl Running {
    /// `bytes` started, under the caps of a plugin on `hooks`, and told `controls`.
    pub(crate) fn start_on(
        bytes: &[u8],
        controls: &[f32],
        hooks: &[Hook],
        permissions: &[PluginPermission],
    ) -> Result<Running, Stop> {
        let most_memory = if hooks.contains(&Hook::Layers) {
            MOST_LAYER_MEMORY
        } else if hooks.contains(&Hook::Inbetween) {
            MOST_BETWEEN_MEMORY
        } else if hooks.contains(&Hook::StrokeSound) {
            MOST_SOUND_MEMORY
        } else if hooks.contains(&Hook::FillMask) {
            MOST_FILL_MEMORY
        } else {
            MOST_MEMORY
        };
        Running::start_within(bytes, controls, most_memory, permissions)
    }

    /// `bytes` started, under its caps, and told `controls`.
    #[cfg(test)]
    pub(crate) fn start(bytes: &[u8], controls: &[f32]) -> Result<Running, Stop> {
        Running::start_within(bytes, controls, MOST_MEMORY, &[])
    }

    /// How much memory it has come to ask for, bytes.
    #[cfg(test)]
    pub(crate) fn memory_used(&self) -> usize {
        self.memory.as_ref().map_or(0, |memory| memory.data(&self.store).len())
    }

    fn start_within(
        bytes: &[u8],
        controls: &[f32],
        most_memory: usize,
        permissions: &[PluginPermission],
    ) -> Result<Running, Stop> {
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
            let mut store = Store::new(&engine, Host::new(limits));
            store.limiter(|host| &mut host.limits);
            store.set_fuel(START_FUEL).map_err(|_| Stop::Damaged)?;
            // the app's functions its permissions give, and nothing else
            let mut linker = Linker::new(&engine);
            crate::plugin_commands::link(&mut linker, permissions).map_err(|_| Stop::Damaged)?;
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
            let picker_pick = instance.get_typed_func(&store, PICKER_PICK).ok();
            let picker_point = instance.get_typed_func(&store, PICKER_POINT).ok();
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
            let action = instance.get_typed_func(&store, ACTION).ok();
            let panel_event = instance.get_typed_func(&store, PANEL_EVENT).ok();
            let event = instance.get_typed_func(&store, EVENT).ok();
            let canvas = (|| {
                Some(CanvasCalls {
                    new: instance.get_typed_func(&store, CANVAS_NEW).ok()?,
                    touch: instance.get_typed_func(&store, CANVAS_TOUCH).ok()?,
                    draw: instance.get_typed_func(&store, CANVAS_DRAW).ok()?,
                    out: instance.get_typed_func(&store, CANVAS_OUT).ok()?,
                    save: instance.get_typed_func(&store, CANVAS_SAVE).ok()?,
                    input: instance.get_typed_func(&store, CANVAS_IN).ok()?,
                    load: instance.get_typed_func(&store, CANVAS_LOAD).ok()?,
                    tool: instance.get_typed_func(&store, CANVAS_TOOL).ok(),
                })
            })();
            let layers = (|| {
                Some(LayerCalls {
                    input: instance.get_typed_func(&store, LAYER_IN).ok()?,
                    out: instance.get_typed_func(&store, LAYER_OUT).ok()?,
                    load: instance.get_typed_func(&store, LAYER_LOAD).ok()?,
                    add: instance.get_typed_func(&store, LAYER_ADD).ok()?,
                    save: instance.get_typed_func(&store, LAYER_SAVE).ok()?,
                    draw: instance.get_typed_func(&store, LAYER_DRAW).ok()?,
                    select: instance.get_typed_func(&store, LAYER_SELECT).ok(),
                    edit: instance.get_typed_func(&store, LAYER_EDIT).ok(),
                    fill: instance.get_typed_func(&store, LAYER_FILL).ok(),
                    export: instance.get_typed_func(&store, LAYER_EXPORT).ok(),
                    hit: instance.get_typed_func(&store, LAYER_HIT).ok(),
                })
            })();
            let key_buffer = instance.get_typed_func(&store, KEY_BUFFER).ok();
            let key_shortcut = instance.get_typed_func(&store, KEY_SHORTCUT).ok();
            let key_shortcut_keys = instance.get_typed_func(&store, KEY_SHORTCUT_KEYS).ok();
            let key_shortcuts = instance.get_typed_func(&store, KEY_SHORTCUTS).ok();
            let panel_key = instance.get_typed_func(&store, PANEL_KEY).ok();
            let kept_buffer = instance.get_typed_func(&store, KEPT_BUFFER).ok();
            let kept = instance.get_typed_func(&store, KEPT).ok();
            let keep = instance.get_typed_func(&store, KEEP).ok();
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
                picker_pick,
                picker_point,
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
                action,
                panel_event,
                event,
                canvas,
                layers,
                key_buffer,
                key_shortcut,
                key_shortcut_keys,
                key_shortcuts,
                panel_key,
                kept_buffer,
                kept,
                keep,
            })
        })?;
        for (index, value) in controls.iter().enumerate() {
            running.control(index, *value)?;
        }
        Ok(running)
    }

    /// `bytes` started as [`Running::start_on`] does, told `controls`, what it `kept` (if it keeps
    /// anything), and on the key hook the app's shortcuts.
    pub(crate) fn start_told(
        bytes: &[u8],
        controls: &[f32],
        hooks: &[Hook],
        permissions: &[PluginPermission],
        kept: Option<&[u8]>,
    ) -> Result<Running, Stop> {
        let mut running = Running::start_on(bytes, controls, hooks, permissions)?;
        if let Some(kept) = kept {
            running.give_kept(kept)?;
        }
        if hooks.contains(&Hook::KeyShortcut) {
            running.tell_shortcuts(&crate::keys::shortcuts_told(crate::keys::DESKTOP))?;
        }
        Ok(running)
    }

    /// `text` written where its key buffer says, for a call that follows: its length.
    fn hand_key_text(&mut self, text: &str) -> Result<i32, Stop> {
        let Running { store, key_buffer, memory, .. } = self;
        let (Some(buffer), Some(memory)) = (key_buffer.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let len = i32::try_from(text.len()).map_err(|_| Stop::Nonsense)?;
        guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            let at = buffer.call(&mut *store, len).map_err(|e| stop_of(&e))?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            memory.write(&mut *store, place, text.as_bytes()).map_err(|_| Stop::Nonsense)?;
            Ok(len)
        })
    }

    /// What the key hook answers for `press` (`keys`: which presses now do what it does by default,
    /// else what it is taken as): none for as it is, else the presses.
    fn key_call(&mut self, keys: bool, press: &crate::KeyCombo) -> Result<Option<Vec<crate::KeyCombo>>, Stop> {
        let func = if keys { self.key_shortcut_keys } else { self.key_shortcut };
        let (Some(func), Some(memory)) = (func, self.memory) else { return Ok(None) };
        let len = self.hand_key_text(&press.text())?;
        let store = &mut self.store;
        let packed = guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            func.call(&mut *store, len).map_err(|e| stop_of(&e))
        })?;
        key_answer(store, &memory, packed)?.map(|text| presses(&text)).transpose()
    }

    /// What it says `press` is taken as: as it is (none), another press, or no press at all (an
    /// empty list). More than one press makes no sense.
    pub(crate) fn key_shortcut(&mut self, press: &crate::KeyCombo) -> Result<Option<Vec<crate::KeyCombo>>, Stop> {
        let taken = self.key_call(false, press)?;
        if taken.as_ref().is_some_and(|presses| presses.len() > 1) {
            return Err(Stop::Nonsense);
        }
        Ok(taken)
    }

    /// The presses it says now do what `press` does by default: as it is (none), or these (perhaps
    /// none).
    pub(crate) fn key_shortcut_keys(&mut self, press: &crate::KeyCombo) -> Result<Option<Vec<crate::KeyCombo>>, Stop> {
        self.key_call(true, press)
    }

    /// Tells it the app's shortcuts (`told`, as [`crate::keys::shortcuts_told`] writes them), if it
    /// listens.
    fn tell_shortcuts(&mut self, told: &str) -> Result<(), Stop> {
        if self.key_shortcuts.is_none() || self.key_buffer.is_none() || self.memory.is_none() {
            return Ok(());
        }
        let len = self.hand_key_text(told)?;
        let Running { store, key_shortcuts, .. } = self;
        let Some(call) = key_shortcuts.as_ref() else { return Ok(()) };
        let (mut left, until) = (TOLD_FUEL, Instant::now() + MOST_TOLD_TIME);
        guarded(|| long_call(store, call, len, &mut left, until))
    }

    /// Whether its panel can be handed a key press: it exports what one goes through.
    pub(crate) fn takes_keys(&self) -> bool {
        self.panel_key.is_some() && self.key_buffer.is_some() && self.memory.is_some()
    }

    /// Tells it the key `press` its panel asked for, on the artwork `session`, as a long call (as a
    /// touch on its panel is).
    pub(crate) fn panel_key(
        &mut self,
        press: &crate::KeyCombo,
        session: std::sync::Arc<crate::CanvasSession>,
    ) -> Result<(), Stop> {
        if !self.takes_keys() {
            return Ok(());
        }
        let len = self.hand_key_text(&press.text())?;
        let Running { store, panel_key, .. } = self;
        let Some(panel_key) = panel_key.as_ref() else { return Ok(()) };
        store.data_mut().session = Some(session as std::sync::Arc<dyn std::any::Any + Send + Sync>);
        let (mut left, until) = (ACTION_FUEL, Instant::now() + MOST_ACTION_TIME);
        let ran = guarded(|| long_call(store, panel_key, len, &mut left, until).map(|_| ()));
        store.data_mut().session = None;
        ran
    }

    /// Hands it what it keeps, if it listens.
    pub(crate) fn give_kept(&mut self, data: &[u8]) -> Result<(), Stop> {
        let Running { store, kept_buffer, kept, memory, .. } = self;
        let (Some(buffer), Some(kept), Some(memory)) = (kept_buffer.as_ref(), kept.as_ref(), memory.as_ref()) else {
            return Ok(());
        };
        let len = i32::try_from(data.len()).map_err(|_| Stop::Nonsense)?;
        let (mut left, until) = (TOLD_FUEL, Instant::now() + MOST_TOLD_TIME);
        guarded(|| {
            let at = long_call(store, buffer, len, &mut left, until)?;
            let place = usize::try_from(at).map_err(|_| Stop::Nonsense)?;
            memory.write(&mut *store, place, data).map_err(|_| Stop::Nonsense)?;
            long_call(store, kept, len, &mut left, until)
        })
    }

    /// What it says to keep from now on, if it has anything new to keep.
    pub(crate) fn take_keep(&mut self) -> Result<Option<Vec<u8>>, Stop> {
        let Running { store, keep, memory, .. } = self;
        let (Some(keep), Some(memory)) = (keep.as_ref(), memory.as_ref()) else { return Ok(None) };
        let (mut left, until) = (TOLD_FUEL, Instant::now() + MOST_TOLD_TIME);
        let packed = guarded(|| long_call(store, keep, (), &mut left, until))?;
        // (nothing new reads as "no press": an empty list)
        if packed < 0 {
            return Ok(None);
        }
        Ok(Some(key_answer(store, memory, packed)?.unwrap_or_default()))
    }

    /// Runs its action `index` on the artwork `session` (what its permissions give it to reach), as a
    /// long call.
    pub(crate) fn action(&mut self, index: usize, session: std::sync::Arc<crate::CanvasSession>) -> Result<(), Stop> {
        let Running { store, action, .. } = self;
        let Some(action) = action.as_ref() else { return Ok(()) };
        let index = i32::try_from(index).map_err(|_| Stop::Nonsense)?;
        store.data_mut().session = Some(session as std::sync::Arc<dyn std::any::Any + Send + Sync>);
        let (mut left, until) = (ACTION_FUEL, Instant::now() + MOST_ACTION_TIME);
        let ran = guarded(|| long_call(store, action, index, &mut left, until).map(|_| ()));
        store.data_mut().session = None;
        ran
    }

    /// Tells it event `kind` happened (1 a stroke kept, 2 a layer added), on the artwork `session`,
    /// to act on as an action does, on a touch's budget: the canvas waits for it.
    pub(crate) fn event(&mut self, kind: i32, session: std::sync::Arc<crate::CanvasSession>) -> Result<(), Stop> {
        let Running { store, event, .. } = self;
        let Some(event) = event.as_ref() else { return Ok(()) };
        store.data_mut().session = Some(session as std::sync::Arc<dyn std::any::Any + Send + Sync>);
        let (mut left, until) = (TOUCH_FUEL, Instant::now() + MOST_TOUCH_TIME);
        let ran = guarded(|| long_call(store, event, kind, &mut left, until).map(|_| ()));
        store.data_mut().session = None;
        // an event shows no panel: one it began is dropped whole
        let host = store.data_mut();
        host.panel = None;
        host.close_panel = false;
        ran
    }

    /// Tells it its panel's row `row` was touched with `value`, on the artwork `session`, as a long
    /// call (as an action is).
    pub(crate) fn panel_event(
        &mut self,
        row: usize,
        value: f32,
        session: std::sync::Arc<crate::CanvasSession>,
    ) -> Result<(), Stop> {
        let Running { store, panel_event, .. } = self;
        let Some(panel_event) = panel_event.as_ref() else { return Ok(()) };
        let row = i32::try_from(row).map_err(|_| Stop::Nonsense)?;
        store.data_mut().session = Some(session as std::sync::Arc<dyn std::any::Any + Send + Sync>);
        let (mut left, until) = (ACTION_FUEL, Instant::now() + MOST_ACTION_TIME);
        let ran = guarded(|| long_call(store, panel_event, (row, value), &mut left, until).map(|_| ()));
        store.data_mut().session = None;
        ran
    }

    /// A new artwork of its own, `width` × `height`: whether it made one (false with no canvas).
    pub(crate) fn canvas_new(&mut self, width: u32, height: u32) -> Result<bool, Stop> {
        let Running { store, canvas, .. } = self;
        let Some(calls) = canvas.as_ref() else { return Ok(false) };
        let size =
            (i32::try_from(width).map_err(|_| Stop::Nonsense)?, i32::try_from(height).map_err(|_| Stop::Nonsense)?);
        let (mut left, until) = (CANVAS_FUEL, Instant::now() + MOST_CANVAS_TIME);
        Ok(guarded(|| long_call(store, &calls.new, size, &mut left, until))? == 0)
    }

    /// A touch on its artwork: whether what it draws changed.
    pub(crate) fn canvas_touch(&mut self, phase: u32, x: f32, y: f32, pressure: f32) -> Result<bool, Stop> {
        let Running { store, canvas, .. } = self;
        let Some(calls) = canvas.as_ref() else { return Ok(false) };
        let (mut left, until) = (TOUCH_FUEL, Instant::now() + MOST_TOUCH_TIME);
        let said = guarded(|| long_call(store, &calls.touch, (phase as i32, x, y, pressure), &mut left, until))?;
        Ok(said != 0)
    }

    /// Tells its artwork the tool (0 paint, 1 erase, 2 smudge), the colour (0xRRGGBBAA), the size
    /// (the brush's width, canvas pixels) and the opacity a stroke starts with.
    pub(crate) fn canvas_tool(&mut self, tool: u32, rgba: u32, size: f32, opacity: f32) -> Result<(), Stop> {
        let Running { store, canvas, .. } = self;
        let Some(call) = canvas.as_ref().and_then(|c| c.tool.as_ref()) else { return Ok(()) };
        guarded(|| {
            store.set_fuel(SAMPLE_FUEL).map_err(|_| Stop::Damaged)?;
            call.call(&mut *store, (tool as i32, rgba as i32, size, opacity)).map_err(|e| stop_of(&e))
        })
    }

    /// What it draws now, as its draw list's bytes.
    pub(crate) fn canvas_draw(&mut self) -> Result<Vec<u8>, Stop> {
        self.canvas_out(|calls| &calls.draw, crate::plugin_canvas::MOST_DATA)
    }

    /// Its artwork as its own data.
    pub(crate) fn canvas_save(&mut self) -> Result<Vec<u8>, Stop> {
        self.canvas_out(|calls| &calls.save, crate::plugin_canvas::MOST_DATA)
    }

    /// What `call` leaves at `procraft_canvas_out()`, as long as it says (at most `most` bytes).
    fn canvas_out(&mut self, call: fn(&CanvasCalls) -> &TypedFunc<(), i32>, most: usize) -> Result<Vec<u8>, Stop> {
        let Running { store, canvas, memory, .. } = self;
        let (Some(calls), Some(memory)) = (canvas.as_ref(), memory.as_ref()) else { return Ok(Vec::new()) };
        let (mut left, until) = (CANVAS_FUEL, Instant::now() + MOST_CANVAS_TIME);
        guarded(|| {
            let len =
                usize::try_from(long_call(store, call(calls), (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            if len > most {
                return Err(Stop::Nonsense);
            }
            let at =
                usize::try_from(long_call(store, &calls.out, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            let mut bytes = vec![0_u8; len];
            memory.read(&*store, at, &mut bytes).map_err(|_| Stop::Nonsense)?;
            Ok(bytes)
        })
    }

    /// `data` handed to it as its artwork: whether it took it.
    pub(crate) fn canvas_load(&mut self, data: &[u8]) -> Result<bool, Stop> {
        let Running { store, canvas, memory, .. } = self;
        let (Some(calls), Some(memory)) = (canvas.as_ref(), memory.as_ref()) else { return Ok(false) };
        let len = i32::try_from(data.len()).map_err(|_| Stop::Nonsense)?;
        let (mut left, until) = (CANVAS_FUEL, Instant::now() + MOST_CANVAS_TIME);
        guarded(|| {
            let at =
                usize::try_from(long_call(store, &calls.input, len, &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            memory.write(&mut *store, at, data).map_err(|_| Stop::Nonsense)?;
            Ok(long_call(store, &calls.load, (), &mut left, until)? == 0)
        })
    }

    /// `bytes` written where its layer calls take what they are handed: their length.
    fn layer_in(&mut self, bytes: &[u8], left: &mut u64, until: Instant) -> Result<i32, Stop> {
        let Running { store, layers, memory, .. } = self;
        let (Some(calls), Some(memory)) = (layers.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let len = i32::try_from(bytes.len()).map_err(|_| Stop::Nonsense)?;
        guarded(|| {
            let at = usize::try_from(long_call(store, &calls.input, len, left, until)?).map_err(|_| Stop::Nonsense)?;
            memory.write(&mut *store, at, bytes).map_err(|_| Stop::Nonsense)?;
            Ok(len)
        })
    }

    /// What `call` (one of its layer calls, answering a length) leaves at `procraft_layer_out()`,
    /// at most `most` bytes.
    fn layer_out(&mut self, call: fn(&LayerCalls) -> &TypedFunc<(), i32>, most: usize) -> Result<Vec<u8>, Stop> {
        let Running { store, layers, memory, .. } = self;
        let (Some(calls), Some(memory)) = (layers.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        guarded(|| {
            let len =
                usize::try_from(long_call(store, call(calls), (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            if len > most {
                return Err(Stop::Nonsense);
            }
            let at =
                usize::try_from(long_call(store, &calls.out, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            let mut bytes = vec![0_u8; len];
            memory.read(&*store, at, &mut bytes).map_err(|_| Stop::Nonsense)?;
            Ok(bytes)
        })
    }

    /// `data` handed to it as a layer of its kind `kind` (none: a new, empty one) to work on:
    /// whether it took it.
    pub(crate) fn layer_load(&mut self, kind: usize, data: &[u8]) -> Result<bool, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(data, &mut left, until)?;
        let kind = i32::try_from(kind).map_err(|_| Stop::Nonsense)?;
        let Running { store, layers, .. } = self;
        let Some(calls) = layers.as_ref() else { return Err(Stop::Damaged) };
        Ok(guarded(|| long_call(store, &calls.load, (kind, len), &mut left, until))? == 0)
    }

    /// `item` recorded on the layer it works on: what it says of it. Any answer but its two makes
    /// no sense.
    pub(crate) fn layer_add(&mut self, item: &[u8]) -> Result<LayerAdded, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(item, &mut left, until)?;
        let Running { store, layers, .. } = self;
        let Some(calls) = layers.as_ref() else { return Err(Stop::Damaged) };
        match guarded(|| long_call(store, &calls.add, len, &mut left, until))? {
            0 => Ok(LayerAdded::Kept),
            1 => Ok(LayerAdded::DrawAgain),
            _ => Err(Stop::Nonsense),
        }
    }

    /// The data of the layer it works on, as it is now (at most `most` bytes).
    pub(crate) fn layer_save(&mut self, most: usize) -> Result<Vec<u8>, Stop> {
        self.layer_out(|calls| &calls.save, most)
    }

    /// The items the layer it works on draws, in order (at most `most` bytes).
    pub(crate) fn layer_draw(&mut self, most: usize) -> Result<Vec<u8>, Stop> {
        self.layer_out(|calls| &calls.draw, most)
    }

    /// Whether it says which items a selection takes and changes them.
    pub(crate) fn layer_edits(&self) -> bool {
        self.layers.as_ref().is_some_and(|calls| calls.select.is_some() && calls.edit.is_some())
    }

    /// The ids of the items of the layer it works on that `area` (a selection) takes, at most
    /// `most` of them.
    pub(crate) fn layer_select(&mut self, area: &[u8], most: usize) -> Result<Vec<u32>, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(area, &mut left, until)?;
        let Running { store, layers, memory, .. } = self;
        let (Some(calls), Some(memory)) = (layers.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let Some(select) = calls.select.as_ref() else { return Ok(Vec::new()) };
        guarded(|| {
            let bytes =
                usize::try_from(long_call(store, select, len, &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            if bytes % 4 != 0 || bytes / 4 > most {
                return Err(Stop::Nonsense);
            }
            let at =
                usize::try_from(long_call(store, &calls.out, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            let mut out = vec![0_u8; bytes];
            memory.read(&*store, at, &mut out).map_err(|_| Stop::Nonsense)?;
            Ok(out.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect())
        })
    }

    /// The changes `changes` asks of the items of the layer it works on, made: whether anything
    /// changed.
    pub(crate) fn layer_edit(&mut self, changes: &[u8]) -> Result<bool, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(changes, &mut left, until)?;
        let Running { store, layers, .. } = self;
        let Some(edit) = layers.as_ref().and_then(|calls| calls.edit.as_ref()) else { return Ok(false) };
        Ok(guarded(|| long_call(store, edit, len, &mut left, until))? != 0)
    }

    /// Whether it takes a colour dropped on its layer.
    pub(crate) fn layer_fills(&self) -> bool {
        self.layers.as_ref().is_some_and(|calls| calls.fill.is_some())
    }

    /// A colour dropped on the layer it works on (`drop`, a record): whether its items changed.
    pub(crate) fn layer_fill(&mut self, drop: &[u8]) -> Result<bool, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(drop, &mut left, until)?;
        let Running { store, layers, .. } = self;
        let Some(fill) = layers.as_ref().and_then(|calls| calls.fill.as_ref()) else { return Ok(false) };
        Ok(guarded(|| long_call(store, fill, len, &mut left, until))? != 0)
    }

    /// The id of the item of the layer it works on at the point `request` (a record) says: none if
    /// there is none there, or it doesn't say (`Ok(None)` either way).
    pub(crate) fn layer_hit(&mut self, request: &[u8]) -> Result<Option<u32>, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(request, &mut left, until)?;
        let Running { store, layers, memory, .. } = self;
        let (Some(calls), Some(memory)) = (layers.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let Some(hit) = calls.hit.as_ref() else { return Ok(None) };
        guarded(|| match long_call(store, hit, len, &mut left, until)? {
            0 => Ok(None),
            4 => {
                let at =
                    usize::try_from(long_call(store, &calls.out, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
                let mut id = [0_u8; 4];
                memory.read(&*store, at, &mut id).map_err(|_| Stop::Nonsense)?;
                Ok(Some(u32::from_le_bytes(id)))
            }
            _ => Err(Stop::Nonsense),
        })
    }

    /// Whether it says which item is at a point.
    pub(crate) fn layer_hits(&self) -> bool {
        self.layers.as_ref().is_some_and(|calls| calls.hit.is_some())
    }

    /// The file it makes of the layers `request` hands it (a record), at most `most` bytes: none if
    /// it makes none.
    pub(crate) fn layer_export(&mut self, request: &[u8], most: usize) -> Result<Option<Vec<u8>>, Stop> {
        let (mut left, until) = (LAYER_FUEL, Instant::now() + MOST_LAYER_TIME);
        let len = self.layer_in(request, &mut left, until)?;
        let Running { store, layers, memory, .. } = self;
        let (Some(calls), Some(memory)) = (layers.as_ref(), memory.as_ref()) else { return Err(Stop::Damaged) };
        let Some(export) = calls.export.as_ref() else { return Ok(None) };
        guarded(|| {
            let made = long_call(store, export, len, &mut left, until)?;
            let Ok(bytes) = usize::try_from(made) else { return Ok(None) };
            if bytes > most {
                return Err(Stop::Nonsense);
            }
            let at =
                usize::try_from(long_call(store, &calls.out, (), &mut left, until)?).map_err(|_| Stop::Nonsense)?;
            let mut out = vec![0_u8; bytes];
            memory.read(&*store, at, &mut out).map_err(|_| Stop::Nonsense)?;
            Ok(Some(out))
        })
    }

    /// The panel it asked to show since last asked (none if it didn't), and whether it asked to
    /// close it.
    pub(crate) fn take_panel(&mut self) -> (Option<crate::PluginPanel>, bool) {
        let host = self.store.data_mut();
        (host.panel.take(), std::mem::take(&mut host.close_panel))
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

    /// The saturation and brightness it says the point (`x`, `y`) of its picker (0..1 each way, y
    /// down) picks; none if it has no picker.
    pub(crate) fn picker_pick(&mut self, x: f32, y: f32) -> Result<Option<(f32, f32)>, Stop> {
        let Running { store, picker_pick, .. } = self;
        let Some(pick) = picker_pick else { return Ok(None) };
        picker_pair(store, pick, (x, y)).map(Some)
    }

    /// Where it says saturation `s` and brightness `v` sit on its picker (0..1 each way, y down);
    /// none if it has no picker.
    pub(crate) fn picker_point(&mut self, s: f32, v: f32) -> Result<Option<(f32, f32)>, Stop> {
        let Running { store, picker_point, .. } = self;
        let Some(point) = picker_point else { return Ok(None) };
        picker_pair(store, point, (s, v)).map(Some)
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

    /// [`can_act`] for code that makes no artworks of its own.
    fn acts(bytes: &[u8], hooks: &[Hook], controls: bool, actions: bool, permissions: &[PluginPermission]) -> bool {
        can_act(bytes, hooks, controls, actions, permissions, false)
    }

    /// A picker that flips each answer upside down: a point (x, y) picks (x, 1 - y), and (s, v) sits at
    /// (s, 1 - v).
    pub(crate) const PICKS: &str = r#"(module
        (func $flip (param f32 f32) (result i64)
            (i64.or
                (i64.extend_i32_u (i32.reinterpret_f32 (local.get 0)))
                (i64.shl (i64.extend_i32_u (i32.reinterpret_f32 (f32.sub (f32.const 1) (local.get 1)))) (i64.const 32))))
        (func (export "procraft_picker_pick") (param f32 f32) (result i64) (call $flip (local.get 0) (local.get 1)))
        (func (export "procraft_picker_point") (param f32 f32) (result i64) (call $flip (local.get 0) (local.get 1))))"#;

    /// An artwork of its own: a dot where it was last touched, drawn as a red square 10 a side; it
    /// saves the dot's place (8 bytes).
    pub(crate) const DOTS: &str = r#"(module
        (memory (export "memory") 1)
        (func (export "procraft_canvas_new") (param i32 i32) (result i32)
            (f32.store (i32.const 0) (f32.const -100)) (f32.store (i32.const 4) (f32.const -100)) (i32.const 0))
        (func (export "procraft_canvas_touch") (param i32 f32 f32 f32) (result i32)
            (f32.store (i32.const 0) (local.get 1)) (f32.store (i32.const 4) (local.get 2)) (i32.const 1))
        (func $word (param $at i32) (param $w i32) (i32.store (local.get $at) (local.get $w)))
        (func $num (param $at i32) (param $n f32) (f32.store (local.get $at) (local.get $n)))
        (func (export "procraft_canvas_draw") (result i32)
            (local $x f32) (local $y f32)
            (local.set $x (f32.load (i32.const 0))) (local.set $y (f32.load (i32.const 4)))
            (call $word (i32.const 1024) (i32.const 1))
            (call $word (i32.const 1028) (i32.const 0xFF0000FF))
            (call $word (i32.const 1032) (i32.const 0))
            (call $num (i32.const 1036) (f32.const 0))
            (call $word (i32.const 1040) (i32.const 0))
            (call $word (i32.const 1044) (i32.const 2))
            (call $num (i32.const 1048) (f32.sub (local.get $x) (f32.const 5)))
            (call $num (i32.const 1052) (f32.sub (local.get $y) (f32.const 5)))
            (call $word (i32.const 1056) (i32.const 3))
            (call $num (i32.const 1060) (f32.add (local.get $x) (f32.const 5)))
            (call $num (i32.const 1064) (f32.sub (local.get $y) (f32.const 5)))
            (call $word (i32.const 1068) (i32.const 3))
            (call $num (i32.const 1072) (f32.add (local.get $x) (f32.const 5)))
            (call $num (i32.const 1076) (f32.add (local.get $y) (f32.const 5)))
            (call $word (i32.const 1080) (i32.const 3))
            (call $num (i32.const 1084) (f32.sub (local.get $x) (f32.const 5)))
            (call $num (i32.const 1088) (f32.add (local.get $y) (f32.const 5)))
            (call $word (i32.const 1092) (i32.const 5))
            (i32.const 72))
        (func (export "procraft_canvas_out") (result i32) (i32.const 1024))
        (func (export "procraft_canvas_save") (result i32)
            (i64.store (i32.const 1024) (i64.load (i32.const 0))) (i32.const 8))
        (func (export "procraft_canvas_in") (param i32) (result i32) (i32.const 2048))
        (func (export "procraft_canvas_load") (result i32)
            (i64.store (i32.const 0) (i64.load (i32.const 2048))) (i32.const 0)))"#;

    /// A plugin's artwork of marks: each touch of a stroke leaves a 10-pixel square in the colour the
    /// stroke started with (its tool told it), a stroke called off takes its marks back, the eraser
    /// takes away every mark within 8 pixels of it, and it keeps and draws all of them (each list
    /// beginning as the last did, while marks are only added). Its data: the count, then each
    /// mark's x, y and colour.
    pub(crate) const MARKS: &str = r#"(module
        (memory (export "memory") 1)
        (func $copy (param $to i32) (param $from i32) (param $len i32)
            (local $i i32)
            (block $done (loop $each
                (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
                (i32.store (i32.add (local.get $to) (local.get $i)) (i32.load (i32.add (local.get $from) (local.get $i))))
                (local.set $i (i32.add (local.get $i) (i32.const 4)))
                (br $each))))
        (func (export "procraft_canvas_new") (param i32 i32) (result i32)
            (i32.store (i32.const 0) (i32.const 0))
            (i32.store (i32.const 4) (i32.const 0x000000FF))
            (i32.store (i32.const 8) (i32.const 0))
            (i32.const 0))
        (func (export "procraft_canvas_tool") (param $tool i32) (param $colour i32) (param f32) (param f32)
            (i32.store (i32.const 8) (local.get $tool))
            (i32.store (i32.const 4) (local.get $colour)))
        (func (export "procraft_canvas_touch") (param $phase i32) (param $x f32) (param $y f32) (param f32) (result i32)
            (local $n i32) (local $i i32) (local $keep i32) (local $at i32) (local $dx f32) (local $dy f32)
            (local.set $n (i32.load (i32.const 0)))
            ;; called off: as many marks as it began with
            (if (i32.eq (local.get $phase) (i32.const 3))
                (then
                    (if (i32.lt_u (i32.load (i32.const 12)) (local.get $n))
                        (then (i32.store (i32.const 0) (i32.load (i32.const 12))) (return (i32.const 1))))
                    (return (i32.const 0))))
            (if (i32.eqz (local.get $phase)) (then (i32.store (i32.const 12) (local.get $n))))
            (if (i32.eq (i32.load (i32.const 8)) (i32.const 1))
                (then
                    (block $done (loop $each
                        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
                        (local.set $at (i32.add (i32.const 64) (i32.mul (local.get $i) (i32.const 12))))
                        (local.set $dx (f32.sub (f32.load (local.get $at)) (local.get $x)))
                        (local.set $dy (f32.sub (f32.load offset=4 (local.get $at)) (local.get $y)))
                        (if (f32.ge (f32.add (f32.mul (local.get $dx) (local.get $dx)) (f32.mul (local.get $dy) (local.get $dy))) (f32.const 64))
                            (then
                                (call $copy (i32.add (i32.const 64) (i32.mul (local.get $keep) (i32.const 12))) (local.get $at) (i32.const 12))
                                (local.set $keep (i32.add (local.get $keep) (i32.const 1)))))
                        (local.set $i (i32.add (local.get $i) (i32.const 1)))
                        (br $each)))
                    (i32.store (i32.const 0) (local.get $keep))
                    (return (i32.ne (local.get $keep) (local.get $n)))))
            (if (i32.ge_u (local.get $n) (i32.const 256)) (then (return (i32.const 0))))
            (local.set $at (i32.add (i32.const 64) (i32.mul (local.get $n) (i32.const 12))))
            (f32.store (local.get $at) (local.get $x))
            (f32.store offset=4 (local.get $at) (local.get $y))
            (i32.store offset=8 (local.get $at) (i32.load (i32.const 4)))
            (i32.store (i32.const 0) (i32.add (local.get $n) (i32.const 1)))
            (i32.const 1))
        (func (export "procraft_canvas_draw") (result i32)
            (local $n i32) (local $i i32) (local $m i32) (local $o i32) (local $x f32) (local $y f32)
            (local.set $n (i32.load (i32.const 0)))
            (local.set $o (i32.const 8192))
            (block $done (loop $each
                (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
                (local.set $m (i32.add (i32.const 64) (i32.mul (local.get $i) (i32.const 12))))
                (local.set $x (f32.load (local.get $m)))
                (local.set $y (f32.load offset=4 (local.get $m)))
                (i32.store (local.get $o) (i32.const 1))
                (i32.store offset=4 (local.get $o) (i32.load offset=8 (local.get $m)))
                (i32.store offset=8 (local.get $o) (i32.const 0))
                (f32.store offset=12 (local.get $o) (f32.const 0))
                (i32.store offset=16 (local.get $o) (i32.const 0))
                (i32.store offset=20 (local.get $o) (i32.const 2))
                (f32.store offset=24 (local.get $o) (f32.sub (local.get $x) (f32.const 5)))
                (f32.store offset=28 (local.get $o) (f32.sub (local.get $y) (f32.const 5)))
                (i32.store offset=32 (local.get $o) (i32.const 3))
                (f32.store offset=36 (local.get $o) (f32.add (local.get $x) (f32.const 5)))
                (f32.store offset=40 (local.get $o) (f32.sub (local.get $y) (f32.const 5)))
                (i32.store offset=44 (local.get $o) (i32.const 3))
                (f32.store offset=48 (local.get $o) (f32.add (local.get $x) (f32.const 5)))
                (f32.store offset=52 (local.get $o) (f32.add (local.get $y) (f32.const 5)))
                (i32.store offset=56 (local.get $o) (i32.const 3))
                (f32.store offset=60 (local.get $o) (f32.sub (local.get $x) (f32.const 5)))
                (f32.store offset=64 (local.get $o) (f32.add (local.get $y) (f32.const 5)))
                (i32.store offset=68 (local.get $o) (i32.const 5))
                (local.set $o (i32.add (local.get $o) (i32.const 72)))
                (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (br $each)))
            (i32.sub (local.get $o) (i32.const 8192)))
        (func (export "procraft_canvas_out") (result i32) (i32.const 8192))
        (func (export "procraft_canvas_save") (result i32)
            (local $len i32)
            (local.set $len (i32.mul (i32.load (i32.const 0)) (i32.const 12)))
            (i32.store (i32.const 8192) (i32.load (i32.const 0)))
            (call $copy (i32.const 8196) (i32.const 64) (local.get $len))
            (i32.add (local.get $len) (i32.const 4)))
        (func (export "procraft_canvas_in") (param i32) (result i32) (i32.const 40000))
        (func (export "procraft_canvas_load") (result i32)
            (local $n i32)
            (local.set $n (i32.load (i32.const 40000)))
            (if (i32.gt_u (local.get $n) (i32.const 256)) (then (return (i32.const 1))))
            (i32.store (i32.const 0) (local.get $n))
            (call $copy (i32.const 64) (i32.const 40004) (i32.mul (local.get $n) (i32.const 12)))
            (i32.const 0)))"#;

    /// A picture: red across, green down, the hue in blue.
    pub(crate) const PICTURE: &str =
        "fn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(at.x, at.y, hue(), 1.0); }";

    /// An action that adds a layer named "Grid" and paints a 10 x 10 red square at (20, 30) on it.
    pub(crate) const ACTS: &str = r#"(module
        (import "procraft" "add_layer" (func $add (result i64)))
        (import "procraft" "write_pixels" (func $write (param i64 i32 i32 i32 i32 i32) (result i32)))
        (import "procraft" "set_layer_name" (func $name (param i64 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "Grid")
        (func (export "procraft_action") (param i32) (result i32)
            (local $layer i64) (local $p i32)
            (local.set $layer (call $add))
            (drop (call $name (local.get $layer) (i32.const 0) (i32.const 4)))
            (local.set $p (i32.const 1024))
            (loop $fill
                (i32.store (local.get $p) (i32.const 0xFF0000FF))
                (local.set $p (i32.add (local.get $p) (i32.const 4)))
                (br_if $fill (i32.lt_u (local.get $p) (i32.const 1424))))
            (drop (call $write (local.get $layer) (i32.const 20) (i32.const 30) (i32.const 10) (i32.const 10) (i32.const 1024)))
            (i32.const 0)))"#;

    #[test]
    fn a_plugin_imports_only_what_its_permissions_give() {
        let bytes = wat::parse_str(ACTS).unwrap();
        let write = [PluginPermission::ArtworkWrite];
        assert!(acts(&bytes, &[], false, true, &write));
        // without the permission it can't run, nor with only others
        assert!(!can_run(&bytes, &[], false));
        assert!(!acts(&bytes, &[], false, true, &[PluginPermission::ArtworkRead, PluginPermission::Brush]));
        // nor what it imports from elsewhere, or with other types
        let importing = |import: &str| {
            let text = format!(
                r#"(module {import} (func (export "procraft_action") (param i32) (result i32) (i32.const 0)))"#
            );
            wat::parse_str(text).unwrap()
        };
        assert!(acts(&importing(r#"(import "procraft" "add_layer" (func (result i64)))"#), &[], false, true, &write));
        assert!(!acts(&importing(r#"(import "env" "add_layer" (func (result i64)))"#), &[], false, true, &write));
        assert!(!acts(&importing(r#"(import "procraft" "add_layer" (func (result i32)))"#), &[], false, true, &write));
        assert!(!acts(&importing(r#"(import "procraft" "memory" (memory 1))"#), &[], false, true, &write));
        // a panel needs its touches' export
        let panel = [PluginPermission::Panel];
        let shows = r#"(import "procraft" "show_panel" (func (param i32 i32) (result i32)))"#;
        assert!(!acts(&importing(shows), &[], false, true, &panel));
        let touches =
            format!(r#"{shows} (func (export "procraft_panel_event") (param i32 f32) (result i32) (i32.const 0))"#);
        assert!(acts(&importing(&touches), &[], false, true, &panel));
        // actions need theirs exported
        let none = position_module("", "(i64.const 0)");
        assert!(acts(&none, &[Hook::Position], false, false, &write));
        assert!(!acts(&none, &[Hook::Position], false, true, &write));
    }

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
            Running::start_on(bytes, &[], hooks, &[]).unwrap().fill(&mut rows, 64, 1, (0, 0))
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
