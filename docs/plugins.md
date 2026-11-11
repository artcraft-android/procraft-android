# Writing a plugin

A plugin is a `.procraft-plugin` file. Someone imports it on the Plugins page, and it has a card there
with Turn on, Turn off and Remove. With none on, the app is as it always was.

Making one doesn't change the app or its code here, and needs no access to this repository: you
build the file on your own computer (see [Building it](#building-it)) and share the file itself.

For now the samples in [`plugins/`](../plugins) also come inside the app, packed: see
[The plugins that come with the app](#the-plugins-that-come-with-the-app).

A plugin holds one or both of these:

- **Code**, which changes how the app behaves. It's a WebAssembly module, run under limits by an
  interpreter, that the app asks at named places called [hooks](#hooks).
- **A [filter](#filters)**, which adds a new adjustment. It's a function the GPU runs over a layer.

The samples are in [`plugins/`](../plugins). `touch-offset` is the smallest with code and `gap-closing`
the largest. `extract-lines`, `posterise` and `outline` are filters.

## What you write

A folder under `plugins/` holding a `craft-extension.json`, and for each thing it holds:

- For code, a Rust crate. Any language that builds to a plain WebAssembly module will do, but
  `tools/pack_plugin.py` builds Rust.
- For a filter, a `filter.wgsl`.

`craft-extension.json` describes it. `tools/pack_plugin.py` lists the code in it for you.

```json
{
  "schema": 1,
  "id": "org.example.nudge",
  "name": "Nudge",
  "version": "1.0.0",
  "holds": ["hooks"],
  "publisher": { "name": "Example" },
  "description": "One sentence on what it does.",
  "compat": { "procraft": { "interface": 1 } },
  "permissions": [],
  "hooks": ["position"],
  "controls": [
    { "key": "across", "type": "slider", "label": "Across", "least": -100, "most": 100, "step": 1,
      "starts": 0, "unit": " px" }
  ]
}
```

`holds` says what's in the file, and the app checks it: `"hooks"` for code, `"filter"` for a filter,
or both. The file has to hold exactly that. The import screen tells people what each one lets the
plugin do.

`hooks` lists the hooks the code uses, and only a plugin that holds `"hooks"` may have it.

`id` must have at least two dotted parts, and can't start with `procraft.` or end with `.stopped`.
`permissions` has to be empty, as there are none to give.

A plugin that only holds a filter starts on, as that changes nothing already in the app. Any other
starts off, until someone taps Turn on. One the app adds by itself starts off, whatever it holds.

## Controls

A plugin can have up to eight, each a `slider` or a `switch`; a plugin with code can have up to eight
`file` controls besides.

| Field | Means |
|---|---|
| `key`, `label` | Its name in the file, and the words beside it |
| `starts` | Its value at first |
| `least`, `most`, `step` | A slider's range and steps |
| `unit` | Written after the value, such as `" px"` |
| `shown_times` | The value is shown multiplied by this: 100 shows .5 as 50 |
| `at_least` | Shown in place of the number at the slider's lowest, such as `"Off"` |
| `names` | A name for each of a slider's steps from its lowest, shown in place of the number |
| `live` | The value goes in as the slider moves, rather than as it lets go |
| `accepts` | What a `file` control takes: `"sound"`, a recording (the app reads it, keeps a minute of it at most with the plugin, and drops it with the plugin) |

A `file` control shows Import, or the name of the file picked and Clear. The plugin is told 1 while a
file is picked, 0 while none is, and is handed the file itself if it takes files (see below); what
the file is for is up to the plugin.

### Rows in the brush studio

A plugin with code can add up to eight rows to the brush studio's Properties page, each a choice of 2
to 16 (each 24 characters at most), kept with each brush:

```json
"brush_controls": [
  { "key": "sound", "label": "Sound", "choices": ["Default", "Eraser", "Pencil"] }
]
```

A brush starts at the first choice. The row shows under its `label` while the plugin is on; a brush
keeps its choice under `plugin:<id>:<key>`, so it stays with the brush when the plugin is off.

### What the code is told

Besides its hooks, code may export any of these, and is told:

- The current brush, before a sound is asked for when it changed: its name and its set's (UTF-8) into
  `procraft_text_buffer(len: i32) -> i32`, which answers where `len` bytes go, then
  `procraft_brush(name_len: i32, set_len: i32)`; and its rows' choices through
  `procraft_brush_control(index: i32, value: f32)`, `index` their place in `brush_controls`.
- The file picked for a `file` control, when it starts and whenever one is picked or let go: into
  `procraft_file_buffer(index: i32, len: i32) -> i32` (where `len` bytes go), then
  `procraft_file(index: i32, len: i32)`, `index` the control's place in `controls` and `len` 0 when
  it is let go. A sound comes as a WAV file of 32-bit floats, one channel, a minute at most. It's a
  long call, on fuel of its own.

A `key` is lowercase letters, digits and `_`, starting with a letter, and can't be `on`. A filter
reads it as a field, so there it also has to be a word WGSL allows, and not a keyword such as `loop`.

Where they show depends on what the plugin holds:

- With code, they're on the plugin's card while it's on. They're told to the code.
- With only a filter, they're on the filter's panel, and start at `starts` each time it opens.
- With both, each place has its own values: the card's go to the code, and the filter's panel still
  starts at `starts`.

## Hooks

A hook is a place in the app that asks a plugin something. All the code can do is hand back an
answer, and the app checks that answer before using it.

### What code can reach

Nothing but the numbers a hook hands it. A module gets no imports at all, so it has no files, no
network, no clock and no layers. A module that asks for an import is refused. A function it exports
for a hook it didn't list is never called.

### The crate

```rust
#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

static ACROSS: AtomicU32 = AtomicU32::new(0);

/// A control's value, by its place in the manifest.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(_index: i32, value: f32) {
    ACROSS.store(value.to_bits(), Ordering::Relaxed);
}

/// Where a sample at (x, y) lands: x's bits in the low half, y's in the high.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_position(x: f32, y: f32) -> i64 {
    let moved = x + f32::from_bits(ACROSS.load(Ordering::Relaxed));
    (u64::from(moved.to_bits()) | u64::from(y.to_bits()) << 32) as i64
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
```

The crate is a `cdylib`, kept out of the app's workspace by an empty `[workspace]` table, and its name
is its folder's. Copy the `Cargo.toml` of a sample: its release profile keeps the module small, and a
module can be 1 MiB at most.

A plugin with code and any controls must export `procraft_control(index: i32, value: f32)`. It's told
each value by its place in the list as the code starts, then each as it changes. A switch is 0 or 1.

Every function a hook names has to be exported, with exactly these types, or the file is refused.

### `position`

Where a stroke's samples land, in window pixels.

```
procraft_position(x: f32, y: f32) -> i64
```

The result holds x's bits in the low half and y's in the high. An answer that isn't a finite number,
or that moves a sample more than 16,384 pixels, stops the plugin. Several plugins on this hook run
one after another, in order of id, each given what the last one said.

### `pick_layer`

Whether the eyedropper reads the current layer (not 0) or the canvas as it looks (0).

```
procraft_pick_layer() -> i32
```

### `interface_scale`

How big the interface shows, where 1 is its usual size, in a window of that many points at 1.

```
procraft_interface_scale(width: f32, height: f32) -> f32
```

An answer outside .5 to 2 stops the plugin. The app may show less than the answer, so that every page
stays in reach on a small window.

### `paste_mask`

Whether a picture pasted with a mask current goes into the mask. If it does, the picture goes through
the plugin a row at a time, and the module must export its `memory`.

```
procraft_paste_mask() -> i32
procraft_row_buffer(bytes: i32) -> i32
procraft_paste_row(at: i32, width: i32)
```

`procraft_row_buffer` says where in `memory` a row of that many bytes can go. Each row is written
there as RGBA, premultiplied, top row first, and `procraft_paste_row` changes its `width` pixels in
place into what the mask should hold. Only one plugin has a picture: the first on the hook.

### `fill_mask`

What a ColorDrop fill keeps. The fill goes through as bits on a thread of its own, so this one may
take a while, and the module must export its `memory`.

```
procraft_fill_mask() -> i32
procraft_fill_begin(width: i32, height: i32, seed_x: i32, seed_y: i32) -> i32
procraft_fill_row(y: i32)
procraft_fill_run() -> i32
procraft_fill_out(y: i32)
```

`procraft_fill_mask` says whether there's anything to do. `procraft_fill_begin` says where in `memory`
a row goes: `width` bits in 64-bit words, low bit first, a set bit being filled. The box has a pixel of
no fill around it, and `seed` is where the fill started. `procraft_fill_row` takes each row as it's
written there, `procraft_fill_run` works it out and says whether anything comes out (not 0), and
`procraft_fill_out` puts each row of what comes out in the same place.

A plugin can only take away: a bit it sets comes out of the fill, and it can't add any. The seed has to
stay in the fill, or the plugin stops.

### `fill_all_layers`

Whether ColorDrop fills by every layer you see (not 0), as if they were one picture, rather than by the
current layer alone (or the reference layer, if one is set).

```
procraft_fill_all_layers() -> i32
```

It only says which to go by: the fill is still the app's, and still goes on the current layer.

### `radial_segments`

How many parts the Drawing Guide's radial symmetry makes, in place of its usual eight.

```
procraft_radial_segments() -> i32
```

An answer outside 2 to 64 stops the plugin. An odd count is made even unless the symmetry only turns
its copies (Rotational Symmetry), since the mirrored copies come in pairs. The count is never written
to the artwork: turn the plugin off, or open the artwork without it, and radial symmetry makes eight
again. With several plugins on this hook, the first by id that answers is used.

### `hide_undo_notice`

Whether undo and redo go by without the notice at the top that names what they took back or brought
back (not 0).

```
procraft_hide_undo_notice() -> i32
```

It only hides the notice: undo and redo work as before.

### `interface_colour`

The colour the interface draws a part of itself in, as 0xRRGGBB, or -1 to leave it as it is.

```
procraft_interface_colour(role: i32) -> i32
```

`role` 0 is the accent: the blue of what is chosen or switched on (a selected tab, a switch, a
slider's filled part, a picked layer). Other roles may come later; answer -1 for any you don't know.
An answer past 0xFFFFFF stops the plugin. With several plugins on this hook, the first by id that
answers is used.

### `pin_panels`

Whether a touch on the canvas with a panel open paints at once and leaves the panel up (not 0), rather
than closing it (a tap) or fading it away (a drag). The panel's own button still closes it.

```
procraft_pin_panels() -> i32
```

### `opacity_together`

Whether a layer's opacity changed while it is one of several picked layers sets every picked layer's
to the same (not 0), as one undo step. A layer that isn't picked still goes alone.

```
procraft_opacity_together() -> i32
```

### `randomise_brush`

While a plugin on this hook is on, the brush studio shows Randomise. Tapping it asks the plugin about
each setting below, by its number, with one `seed` for the whole tap:

```
procraft_randomise(setting: i32, seed: i32) -> f32
```

Answer where the setting goes as a share of its range, 0 to 1 (0 its least, 1 its most), or anything
below 0 to leave it as it is. An answer above 1, or not a number, stops the plugin. A whole-number
setting (a choice or a switch) is rounded. The plugin gets no randomness of its own: mix the seed and
the setting's number to get one.

| Number | Setting | Range |
|---|---|---|
| 0 | `spacing` | 0 to 1 |
| 1 | `streamline` | 0 to 1 |
| 2 | `streamline_pressure` | 0 to 1 |
| 3 | `pen_taper_start` | 0 to 1 |
| 4 | `pen_taper_end` | 0 to 1 |
| 5 | `pen_taper_size` | 0 to 1 |
| 6 | `pen_taper_opacity` | 0 to 1 |
| 7 | `touch_taper_start` | 0 to 1 |
| 8 | `touch_taper_end` | 0 to 1 |
| 9 | `touch_taper_size` | 0 to 1 |
| 10 | `touch_taper_opacity` | 0 to 1 |
| 11 | `input_style` | 0 to 2 (whole) |
| 12 | `shape_rotation` | -1 to 1 |
| 13 | `scatter` | 0 to 1 |
| 14 | `count` | 0 to 1 |
| 15 | `randomized` | 0 to 1 (whole) |
| 16 | `roundness` | 0 to 1 |
| 17 | `shape_angle` | 0 to 1 |
| 18 | `grain_behavior` | 0 to 1 (whole) |
| 19 | `grain_scale` | 0 to 1 |
| 20 | `grain_depth` | 0 to 1 |
| 21 | `rendering_mode` | 0 to 5 (whole) |
| 22 | `flow` | 0 to 1 |
| 23 | `jitter_size` | 0 to 1 |
| 24 | `jitter_opacity` | 0 to 1 |
| 25 | `pressure_size` | -1 to 1 |
| 26 | `pressure_opacity` | -1 to 1 |
| 27 | `tilt_angle` | 0 to 1 |
| 28 | `tilt_opacity` | 0 to 1 |
| 29 | `tilt_size` | 0 to 1 |
| 30 | `max_size` | 0 to 1 |
| 31 | `min_size` | 0 to 1 |
| 32 | `max_opacity` | 0 to 1 |
| 33 | `min_opacity` | 0 to 1 |
| 34 | `smudge_pull` | 0 to 1 |
| 35 | `wet_pull` | 0 to 1 |

Randomise is one change to the brush: Cancel or Reset brush takes it back.

### `colour_triangle`

Whether the colour panel's Classic tab shows a triangle in place of its square (not 0): saturation and
brightness between the pure hue (its right corner), white (top left) and black (bottom left). The hue
slider and the others under it stay as they are.

```
procraft_colour_triangle() -> i32
```

### `inbetween`

Animation Assist's frame options get In-between while a plugin is on this hook: the plugin draws a
frame between the current frame and the next (each a single layer), and the app puts it just after
the current one and makes it current, as one undo step. Tapped again, it goes between the new frame
and the next, and so on. How the frame between looks is all the plugin's: the app only hands over the
two frames' pixels (the part either is drawn in) and takes back what it draws.

```
procraft_between_begin(width: i32, height: i32) -> i32
procraft_between_row(frame: i32, y: i32)
procraft_between_run() -> i32
procraft_between_out(y: i32)
```

`begin` starts one of `width` × `height` pixels and answers where in its memory a row of them goes
(`width` × 4 bytes, premultiplied RGBA). The app then writes each row of this frame (`frame` 0) and of
the next (`frame` 1) there, calling `row` after each; calls `run`, which draws the frame between (0 for
none); and for each row calls `out` and reads it back from the same place. It is a long call, on fuel
by the area, with up to 256 MB of memory; one that runs out stops. The Tweening plugin the app comes
with slides each drawing from its own place and size toward the other's and fades between them.

### `quickshape_curves`

Whether a stroke that bends more than once snaps to a curve (not 0): a chain of curves through its
bends, with an anchor at each and two controls pulling on the curve either side. The polyline it would
have been is still among the kinds to switch to. Dragging a control moves its pair the other way, so
the curve stays smooth there; tapping an anchor sets it free (each control on its own), then a corner,
then smooth again.

```
procraft_quickshape_curves() -> i32
```

### `stroke_sound`

A sound as you draw. While a plugin on this hook is on, the app asks it for the stroke's sound every
few milliseconds, on a thread of its own, and plays what it hands back:

```
procraft_stroke_sound(frames: i32, rate: i32, speed: f32, pressure: f32, since_ms: f32,
                      state: i32, hover_x: f32, hover_y: f32, height: f32) -> i32
procraft_sound_prepare(rate: i32) -> i32
```

It answers where in its memory the next `frames` samples are (32-bit floats, -1 to 1, at `rate` a
second). The stroke moves at `speed` (window heights a second) with `pressure` (a finger as pressed
all the way); its last input came `since_ms` ago (below 0: no stroke is open); `state` says, as bits,
1 a stroke is open, 2 the pen hovers (at `hover_x`, `hover_y`, window pixels, in a window `height`
tall), 4 the tool erases. Before the first sound at a rate, `procraft_sound_prepare` (if it has one)
readies whatever it makes once, as a long call. A sound may spend fuel by the frame; there's 128 MB
of memory. It's told the brush and its rows as above, so a sound can go by the brush.

The Brush sounds plugin the app comes with behaves as Krita's Brush SFX plugin by davi133 does
(github.com/davi133/brush_sfx): six sounds (an eraser, a pencil, a pen, a paintbrush, an airbrush and
a spray can), each brush the one its kind takes (by its name, else its set's), louder the faster and
harder the stroke goes, the pen's and the paintbrush's ring rising with speed, the airbrush hissing
while pressed and the spray can while it moves, its ball rattling when the pen is shaken back and
forth above the canvas. Its controls set the volume, one sound for every brush, whether erasing makes
a sound and which, and a recording for any of the six; its row in the brush studio gives a brush its
own sound, or none. All of it is in the plugin, made sample by sample with a few filters.

### Limits

| | |
|---|---|
| The module | 1 MiB |
| Memory it may ask for | 16 MiB, or 64 MiB on `fill_mask` |
| Calls deep | 256 |
| Steps to start | About 1,000,000 |
| Steps for a sample, a control or a question | About 100,000 |
| Steps for a pasted row | 10,000, and 64 for each pixel |
| Steps for a fill | 20,000,000, and 1,024 for each pixel of its box, for at most 30 seconds |
| A fill's box | 8,194 × 8,194 pixels in area |

A pen sends a sample every 4 ms at 240 Hz, so a `position` call has to be far quicker than that. Use a
few hundred steps, not thousands.

A fill too big for the limit stays as it was, and the app says so.

Code keeps its memory while the plugin is on, between calls. It starts afresh when the plugin is
turned on, and on each start of the app, so anything it remembers lasts only that long.

## Filters

A filter shows under Adjustments in a Plugins row while its plugin is on. It redraws the current layer
live, Apply keeps that as one undo step, and Cancel puts the layer back. What it did is pixels, so an
artwork never needs the plugin again.

### The function

`filter.wgsl` holds one function, and any helpers, constants or structs it needs:

```wgsl
fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let dark = 1.0 - dot(colour.rgb, vec3<f32>(0.299, 0.587, 0.114));
    return vec4<f32>(vec3<f32>(0.0), step(v.cut, dark) * colour.a);
}
```

- `colour` is the layer's pixel, in straight colour, not premultiplied.
- `at` is where it is, in canvas pixels, top left at (0, 0). A pixel's centre is at .5.
- It returns the new pixel, in straight colour. The app clamps it to 0..1.
- It has to be called `pixel`. WGSL keeps the word `filter` for itself, so a function can't have that name.

You're given three things:

| | |
|---|---|
| `v.<key>` | Each control's value, as an `f32`. A switch is 0 or 1 |
| `layer_at(at)` | The layer as it was at `at`, straight colour. Past an edge, the edge's pixel |
| `canvas_size()` | The canvas's width and height, in pixels |

The app does the rest: premultiplying, the selection, the alpha lock and the copy of the layer. A
filter can't see the selection, and doesn't need to.

Its manifest is like the one above, with `"holds": ["filter"]`, no `hooks`, and its controls:

```json
"controls": [
  { "key": "cut", "type": "slider", "label": "Cut-off", "least": 0, "most": 1, "step": 0.01,
    "starts": 0.5, "shown_times": 100, "unit": "%", "live": true },
  { "key": "keep_colour", "type": "switch", "label": "Keep the colour", "starts": false }
]
```

### What is refused

The file is read before anything runs. It's refused if the text is over 64 KiB, doesn't parse, or
has more than eight controls. So is anything a filter has no use for:

- Entry points, bindings, `var` at module scope, or `override`.
- Anything that reads or writes outside the function: textures, buffers, atomics, barriers.
- `discard`, and the derivatives `dpdx`, `dpdy` and `fwidth`.
- A module-scope name the app's own code uses, such as a function called `mix`, or any name
  starting with `host_`. Inside a function, a name is your own.
- A type over 16 KiB, or a function's variables over 64 KiB. Each pixel has its own, so large
  arrays stop a GPU.
- 64-bit and 16-bit numbers, which some GPUs don't have.

### How long it may take

A GPU can't be interrupted, and a shader that runs too long can stop every app on the device. So a
pixel has a budget: about 65,000 simple operations in loops, where `sin` or `pow` counts as four and
`layer_at()` as about twenty, and as much again outside them. A 31 by 31 neighbourhood fits. Every loop
pays for each round as it goes, and stops when the budget is spent.

Before a filter with a loop touches the layer, the app tries it on a few hundred pixels spread over
the layer. If one of them runs out, the plugin is switched off, saying it took too long, and the
layer isn't touched. The same goes for a draw that takes far longer than it can have, or a driver
that fails on it.

A filter with no loop is drawn in one go. One with a loop is drawn in bands, so the screen keeps
going. If a whole draw takes more than about 0.15 seconds, sliders stop following the finger and
redraw as it lets go. If it takes more than 4 seconds, the filter is closed and the layer put back.

## When a plugin stops

It's switched off at once, all of it, and its card says why. It stays off through a restart until
someone taps Turn on.

Code stops if it runs out of steps, asks for too much memory, traps, or gives an answer that makes no
sense. Whatever it was handed goes on as it came.

## Building it

1. Get a copy of this repository: `git clone https://github.com/artcraft-android/procraft`, or
   Code > Download ZIP on its GitHub page. It stays on your computer; nothing you do in it comes
   back here.
2. For code, install Rust (from rustup.rs). Python 3 runs the packer.
3. Put your plugin's folder under `plugins/` in that copy, then, from the copy's top folder:

```
rustup target add wasm32-unknown-unknown
python tools/pack_plugin.py nudge --out somewhere
```

That writes `nudge.procraft-plugin`. Only code needs the `rustup` line. To try it, import it through
the Plugins page, and tap Turn on if it starts off. A file packed again from the same folder comes out
the same, byte for byte.

## The plugins that come with the app

Every plugin under `plugins/` comes inside the app, packed, in `crates/core/bundled-plugins/`, and
`crates/core/src/bundled.rs` lists them. The first time the app comes with one, it adds it as if
someone had imported the file, but off, whatever it holds. From then on it's a plugin like any
other, with the same card, Turn on and Remove.

- The app writes down in its plugin folder (`added`) each one it added, so one that's removed stays
  removed, through restarts and updates. Importing its file brings it back, as any import.
- A later version of the app adds any it never added before, and puts a newer file in place of an
  older one that's there, keeping what was set, as an import of it would.
- One imported before the app came with it is left as it is, but for a newer file.
- A setting the app used to have and a plugin now `keeps` (ColorDrop gap closing's gap, Interface
  size, Eyedropper picks from layer, Paste into masks) starts the plugin the app added on with it, if
  someone had changed it. Once the plugin is turned on or off, that is what's kept.

The packed files are committed, so building the app needs no WebAssembly target. Whenever anything
under `plugins/` changes, pack them again and commit what changed:

```
python tools/pack_plugin.py --bundle
```

That also writes `sources.txt` beside them, a digest of each folder as it was packed. The core's
tests (`cargo test -p procraft-core bundled`) check each packed file against its folder, the digest
too, so a change not packed again fails them. A new plugin needs its line in `bundled.rs` as well.

To ship the app without them, take out `bundled.rs` (its test with it), its `mod` line and the folder,
and have `Plugins::with_bundled` add none. What was added stays, as if imported.

## When the app changes

`interface` in `compat` is the version of all this: the hooks and what they call, the template around
a filter's function, what a filter is given, and the kinds of control. What a hook calls stays as it's
written here, and the app keeps every interface's template, so a plugin goes on working as it was
written.

Something that needs more uses a newer interface number, and an older app says it needs a newer
ProCraft. So does an app that doesn't have a hook a plugin lists, or something `holds` names.
