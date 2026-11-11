# Writing a plugin

A plugin is a `.procraft-plugin` file. Someone imports it on the Plugins page, and it has a card there
with Turn on, Turn off and Remove. With none on, the app is as it always was.

Making one doesn't change the app or its code here, and needs no access to this repository: you
build the file on your own computer (see [Building it](#building-it)) and share the file itself.

For now the samples in [`plugins/`](../plugins) also come inside the app, packed: see
[The plugins that come with the app](#the-plugins-that-come-with-the-app).

A plugin holds one or both of these:

- **Code**, which changes how the app behaves. It's a WebAssembly module, run under limits by an
  interpreter, that the app asks at named places called [hooks](#hooks). Code can also add buttons
  that change the artwork ([actions](#actions)), show a [panel](#panels) of its own, draw the colour
  panel's Classic picker ([`picker`](#picker)), or make a kind of artwork of its own
  ([artworks of its own](#artworks-of-its-own)).
- **A [filter](#filters)**, which adds a new adjustment. It's a function the GPU runs over a layer.

The samples are in [`plugins/`](../plugins). `touch-offset` is the smallest with code and `gap-closing`
the largest. `extract-lines`, `posterise` and `outline` are filters. `grid-layer` has an action,
`colour-triangle` is a picker, and `vector-canvas` makes artworks of its own.

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

`hooks` lists the hooks the code uses, and only a plugin that holds `"hooks"` may have it. Code
needs at least one hook, one action (see [Actions](#actions)) or one kind of artwork (see
[Artworks of its own](#artworks-of-its-own)). `interface` is 1 here; actions, permissions, panels,
the `picker` hook and kinds of artwork need 2, and a plugin of switches (no code) needs 3.

`id` must have at least two dotted parts, and can't start with `procraft.` or end with `.stopped`.
`permissions` lists what code may reach when someone taps one of its actions. Leave it empty unless
the plugin has actions. Only code may ask, and a permission this app doesn't give is refused.

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

A hook is a place in the app that asks a plugin something. At a hook, all the code can do is hand
back an answer, and the app checks that answer before using it. Changing the artwork is for
[actions](#actions), and for the two hooks that tell a plugin something happened,
[`stroke_kept` and `layer_added`](#stroke_kept-and-layer_added), which act as actions do.

### What code can reach

At a hook, nothing but the numbers the hook hands it (the two that tell of an event: the functions
its permissions give, as an action). A module imports only the app's functions its
`permissions` give (see [Actions](#actions)), so it has no files, no network and no clock. A module
that imports anything else is refused. A function it exports for a hook it didn't list is never
called.

### Plugins of switches

A plugin that only says yes, or a number, at hooks that hand it nothing needs no code: it holds
`hooks`, lists no `module.wasm`, and each of its hooks answers yes while it is on, or the value of a
control it names in `answers`:

```json
"holds": ["hooks"],
"hooks": ["radial_segments"],
"controls": [{ "key": "parts", "type": "slider", "label": "Segments", "least": 2, "most": 32, "step": 1, "starts": 8 }],
"answers": { "radial_segments": "parts" }
```

The hooks it may be on: `pick_layer`, `fill_all_layers`, `hide_undo_notice`, `pin_panels`,
`opacity_together` and `colour_triangle` (yes while on, or a switch's value), and `radial_segments`
(a slider's value, which it has to name). It can have no actions, kinds of artwork, brush studio rows,
file controls or permissions: those are for code. The packer leaves a plugin folder with no
`Cargo.toml` uncompiled, and `interface` is 3. Six of the plugins that come with the app are plugins
of switches.

### The crate

The plugin kit in `plugins/kit` writes the calls for you, so a plugin is ordinary Rust: its controls
kept as the app tells them, and a macro for each hook that hands pictures, sounds, text and files back
and forth.

```toml
[dependencies]
kit = { package = "procraft-plugin-kit", path = "../kit" }
```

```rust
/// The controls, by their place in the manifest, as they start.
static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);

// where a sample lands: moved across by the control
kit::position!(|x, y| (x + CONTROLS.get(0), y));
```

The macros: `controls!` (with a closure, also told each control as it changes), `yes!` for hooks that
only ask yes or no (`kit::yes!(procraft_pin_panels)`), `answer!` for a whole number, `position!`, `stroke_sound!` and
`sound_prepare!`, `brush!` and `brush_control!`, `files!`, `inbetween!`, `quickshape_curve!` and `events!`. The
kit uses the standard library; for the smallest plugins, `default-features = false` keeps only
`Controls`, `controls!`, `yes!`, `answer!` (a whole number) and `position!`, and the crate is `#![no_std]` with a panic handler of its own:

```rust
#![no_std]

static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);
kit::yes!(procraft_pin_panels, || CONTROLS.on(0));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
```

Without the kit, a plugin exports the functions itself, with exactly the types each hook below names.
A plugin built with the kit is packed again whenever the kit changes (its packed file is checked
against the kit too).

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
slider and the others under it stay as they are. It stays for plugins written to it; `picker` does
this and more.

```
procraft_colour_triangle() -> i32
```

### `picker`

The plugin draws the colour panel's Classic square its own way (interface 2). It brings the picture
as WGSL in `picture.wgsl`, beside its module, which the app draws on the GPU each time the hue changes.
Its code says where a touch lands and where the knob sits. Points go 0 to 1 across and down the
square, and saturation and brightness go 0 to 1. Each pair comes back packed as `position`'s do.

```
procraft_picker_pick(x: f32, y: f32) -> i64     // the saturation and brightness at (x, y)
procraft_picker_point(s: f32, v: f32) -> i64    // where they sit
```

`picture.wgsl` has one function, `picture`. It's asked for the colour (straight, not premultiplied)
at each pixel's centre, and given `hue()` (0 to 1 round the wheel from red) and `picture_size()` in
pixels:

```wgsl
fn picture(at: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(at.x, 1.0 - at.y, hue(), 1.0);
}
```

It's checked as a filter's function is (see [What is refused](#what-is-refused)): it reaches
nothing, and its loops pay from a budget. A picture that runs out of budget, or takes too long,
switches the plugin off. `tools/pack_plugin.py` packs `picture.wgsl` for a plugin on this hook. The
kit's `picker!` writes the two functions, and `plugins/colour-triangle` is one to start from.

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

### `stroke_kept` and `layer_added`

Told each time a stroke is kept (`stroke_kept`), or a layer is added with the Layers panel's +
(`layer_added`), the plugin acts as an action does, with the functions its permissions give (see
[Actions](#actions)):

```
procraft_event(kind: i32) -> i32
```

`kind` is 1 for a stroke kept, 2 for a layer added. It runs at once, before the next touch is taken,
on a touch's budget (it should be quick: one that runs out goes off and says why), and what it changes
is one undo step of its own, after the stroke's or the layer's. What a plugin changes itself, here or
in an action, tells no plugin anything, so they can't set each other off. The kit's `events!` takes
a `fn(kit::app::Event)`.

### `quickshape_curve`

A curve QuickShape may snap to. When a held stroke snaps, the app hands the plugin the stroke's points
and the kind QuickShape found first; the plugin may offer a curve, which then comes first among the
kinds to switch to (what QuickShape found still there). When Curve is picked from the kinds again,
the plugin is asked for one by name. The app draws the curve and lets it be reshaped: dragging a
control moves its pair the other way, so the curve stays smooth there; tapping an anchor sets it free
(each control on its own), then a corner, then smooth again.

```
procraft_curve_begin(count: i32, best: i32) -> i32
procraft_curve_run() -> i32
procraft_curve_nodes() -> i32
```

`begin` answers where the stroke's `count` points go (x and y, 32-bit floats each, window pixels);
`best` is the kind QuickShape found first, by its place among line, polyline, arc, curve, ellipse,
circle, triangle, equilateral triangle, quadrilateral, rectangle and square, or -1 when a curve is
asked for by name. `run` answers how many nodes the curve has, 0 for none: an anchor, then two
controls and an anchor for each span (4, 7, 10 ...). `nodes` answers where they are, as the points
came. The QuickShape curves plugin the app comes with offers one for a polyline: an anchor at each
corner, each anchor's controls leaning the way the stroke came and went.

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

## Actions

An action is something code does when tapped: a row on its card, under its controls, once it's on.
While it runs, it can call the app's functions its `permissions` give: read the artwork, add layers
and paint on them, set the brush, show a short message. Everything one action changes is one step
undo takes back.

```json
  "compat": { "procraft": { "interface": 2 } },
  "permissions": ["artwork.read", "artwork.write", "brush"],
  "hooks": [],
  "actions": [
    { "key": "add", "label": "Add a grid layer" }
  ]
```

A plugin can have up to eight actions. Each `key` is used once, and each `label` is the row's words.
Its card lists them all; `"place"` lists one somewhere else too:

- `"adjustments"`: in the Adjustments menu's Plugins list, after the filters, for something done
  to the artwork as an adjustment is;
- `"layer"`: in the current layer's options, after the app's own rows;
- `"frame"`: in Animation Assist's frame options, after the app's own rows, run with that frame's
  layer current.

A place this app doesn't have makes the file one for a newer ProCraft. Actions need interface 2. The import screen
says what each permission lets the plugin do.

| Permission | The functions it gives |
|---|---|
| `artwork.read` | `canvas_size`, `layer_count`, `layer_at`, `current_layer`, `read_pixels` |
| `artwork.write` | `add_layer`, `write_pixels`, `set_layer_name`, `set_layer_opacity`, `set_layer_visible` |
| `brush` | `brush_colour`, `set_brush_colour`, `brush_size`, `set_brush_size`, `brush_opacity`, `set_brush_opacity` |
| `notice` | `notice` |
| `panel` | `show_panel`, `close_panel` (see [Panels](#panels)) |

The module imports them from `"procraft"`, and the app checks each one's types. It exports
`procraft_action(index: i32) -> i32`, called with the action's place in `actions`; it returns 0.
The kit writes all of this:

```rust
use kit::app;

kit::actions!(|_| {
    let (width, height) = app::canvas_size();
    if let Some(layer) = app::add_layer() {
        app::name_layer(layer, "Frame");
        let ink = [0, 0, 0, 255].repeat(width as usize * 4);
        app::paint(layer, 0, 0, width, 4, &ink);
        app::paint(layer, 0, height as i32 - 4, width, 4, &ink);
    }
});
```

- Layers go by the id the app gives each one, never 0. 0 means none.
- Pixels are premultiplied RGBA8, rows from the top. A rectangle has to be wholly on the canvas, at
  most 4,096 × 4,096 pixels. A write goes over what was there.
- Code can paint only on a pixel layer that isn't locked.
- Sizes and opacities are 0 to 1, as the sliders show them. A colour is `0xRRGGBB`.
- A name or a message is UTF-8 text, at most 200 characters.

An action runs in a copy of the code of its own, told the controls as they are, so its hooks go on
meanwhile. It may take 4,000,000,000 steps, for at most 20 seconds. Calls to the app's functions
answer only while an action runs. At a hook they do nothing.

`plugins/grid-layer` is one to start from.

### Panels

With the `panel` permission, an action can show a panel of the plugin's own. Each app draws it the
way its own cards are drawn: a title and Done, then rows. The rows are a slider, a switch, a choice,
some words, or a button. The plugin describes the panel as JSON text and hands it to
`show_panel(at, len)`, which answers 1 if the panel holds up and 0 if not:

```json
{ "title": "Halftone", "rows": [
  { "type": "slider", "label": "Dot size", "least": 1, "most": 50, "step": 1, "value": 8, "unit": " px" },
  { "type": "switch", "label": "Round dots", "on": true },
  { "type": "choice", "label": "Shape", "options": ["Dots", "Lines"], "chosen": 0 },
  { "type": "text", "text": "Works on the current layer." },
  { "type": "button", "label": "Apply" }
] }
```

- A panel has 1 to 16 rows, and a choice has 1 to 8 options.
- Labels and the title are at most 40 characters, an option at most 24, and words at most 400.
- A field the app doesn't know, or a kind of row it doesn't have, makes the panel not hold up.
- The panel shows once the action ends, and stays open in the same copy of the code.
- Each touch calls the module's `procraft_panel_event(row: i32, value: f32) -> i32`, so a module
  with the `panel` permission has to export it. `row` is the row's place, from 0. The value is the
  slider's (on its steps), 1 or 0 for a switch, the option's place for a choice, or 1 for a button.
  A slider is told when it's let go.
- A touch can do what an action can, and what it changes is one step to undo. It can show the panel
  again, changed, or close it with `close_panel()`.
- Done closes the panel. So does another action, or the plugin being turned off.

The kit writes the JSON for you:

```rust
use kit::app::{self, Panel};

kit::actions!(|_| {
    Panel::new("Halftone").slider("Dot size", 1.0, 50.0, 1.0, 8.0, " px").button("Apply").show();
});
kit::panel!(|row, value| {
    if row == 1 {
        apply_halftone();
        app::close_panel();
    }
});
```

## Artworks of its own

Code can make a kind of artwork of its own: a vector drawing, say. The plugin keeps the artwork in
its own format. The app hands it the strokes, shows what it draws, and keeps what it saves after each
stroke as a step for undo and redo. The manifest lists the kinds (interface 2):

```json
  "kinds": [{ "key": "vector", "label": "Vector artwork" }]
```

While the plugin is on, the gallery's New canvas list has a row for each kind under the presets,
the screen's size. Such an artwork's folder keeps the plugin's data in `plugin-data.bin`, its kind in
`artwork.json`, and what it last drew as its layer. Opened with the plugin off, it shows that, and the
plugin's data stays as it was. Share Image offers SVG for it too, written from its draw list, so its
shapes stay shapes. A plugin can list up to four kinds. Its module exports:

```
procraft_canvas_new(width: i32, height: i32) -> i32           // 0 if it made one that size
procraft_canvas_touch(phase: i32, x: f32, y: f32, pressure: f32) -> i32
procraft_canvas_draw() -> i32                                  // the draw list's length
procraft_canvas_save() -> i32                                  // its data's length
procraft_canvas_out() -> i32                                   // where either of those is
procraft_canvas_in(len: i32) -> i32                            // where `len` bytes of data go
procraft_canvas_load() -> i32                                  // 0 if it took them
procraft_canvas_tool(tool: i32, colour: i32, size: f32, opacity: f32)   // if it exports it
```

`procraft_canvas_tool` is told what each stroke starts with: the tool (0 paint, 1 erase, 2 smudge),
the colour (`0xRRGGBBAA`), the brush's width in canvas pixels, and its opacity (0 to 1).

- A touch's `phase` is 0 down, 1 moved, 2 up, or 3 called off (a second finger made it a gesture).
  Coordinates are canvas pixels. The touch answers not 0 if what it draws changed.
- The draw list is little-endian 32-bit words. `1 fill stroke width flags` starts a path. Fill and
  stroke are `0xRRGGBBAA`, straight, with alpha 0 for none. The width is a float. Flags 1 makes the
  ends and corners round, and 2 fills by the even-odd rule. Then `2 x y` moves, `3 x y` draws a line,
  `4 x1 y1 x2 y2 x y` draws a cubic curve, and `5` closes. A list that doesn't hold up stops the
  plugin.
- Data is at most 64 MiB. A touch may take 20,000,000 steps, for at most a quarter of a second; the
  other calls 800,000,000, for at most 5 seconds.

The kit's `canvas` module writes it: an `Artwork` (new, the brush a stroke starts with, touch, draw
into a `DrawList`, save, load) and `kit::canvas!(MyArtwork)`. `plugins/vector-canvas` is one to start
from.

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
someone taps Turn on. An action that stops partway leaves what it did so far as one step to undo.

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
a filter's function, what a filter is given, the kinds of control, and the app's functions. Interface
2 added actions, permissions, panels, the picture of the `picker` hook and kinds of artwork, and
interface 3 plugins of switches. What a
hook calls stays as it's written here, and the app keeps every interface's template, so a plugin goes
on working as it was written.

Something that needs more uses a newer interface number, and an older app says it needs a newer
ProCraft. So does an app that doesn't have a hook a plugin lists, or something `holds` names.
