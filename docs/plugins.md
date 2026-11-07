# Writing a plugin

A plugin is a `.procraft-plugin` file. Someone imports it on the Plugins page, and it has a card there
with Turn on, Turn off and Remove. None of a plugin's code is in the app, so with none on, the app is
as it always was.

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
starts off, until someone taps Turn on.

## Controls

A plugin can have up to eight, each a `slider` or a `switch`.

| Field | Means |
|---|---|
| `key`, `label` | Its name in the file, and the words beside it |
| `starts` | Its value at first |
| `least`, `most`, `step` | A slider's range and steps |
| `unit` | Written after the value, such as `" px"` |
| `shown_times` | The value is shown multiplied by this: 100 shows .5 as 50 |
| `at_least` | Shown in place of the number at the slider's lowest, such as `"Off"` |
| `live` | The value goes in as the slider moves, rather than as it lets go |

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

```
rustup target add wasm32-unknown-unknown
python tools/pack_plugin.py nudge --out somewhere
```

That writes `nudge.procraft-plugin`. Only code needs the `rustup` line. To try it, import it through
the Plugins page, and tap Turn on if it starts off.

## When the app changes

`interface` in `compat` is the version of all this: the hooks and what they call, the template around
a filter's function, what a filter is given, and the kinds of control. What a hook calls stays as it's
written here, and the app keeps every interface's template, so a plugin goes on working as it was
written.

Something that needs more uses a newer interface number, and an older app says it needs a newer
ProCraft. So does an app that doesn't have a hook a plugin lists, or something `holds` names.
