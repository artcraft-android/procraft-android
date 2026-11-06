# Writing a filter plugin

A filter is a `.procraftplugin` file that adds a new adjustment. Someone imports it on the Plugins
page, and it shows under Adjustments in a Plugins row. It redraws the current layer live, Apply keeps
that as one undo step, and Cancel puts the layer back. What it did is pixels, so an artwork never
needs the plugin again.

The samples are in [`plugins/`](../plugins): `extract-lines`, `posterise` and `outline`.

## What you write

A folder under `plugins/` with two files. `filter.wgsl` holds one function, and any helpers,
constants or structs it needs:

```wgsl
fn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let dark = 1.0 - dot(colour.rgb, vec3<f32>(0.299, 0.587, 0.114));
    return vec4<f32>(vec3<f32>(0.0), step(v.cut, dark) * colour.a);
}
```

- `colour` is the layer's pixel, in straight colour, not premultiplied.
- `at` is where it is, in canvas pixels, top left at (0, 0). A pixel's centre is at .5.
- It returns the new pixel, in straight colour. The app clamps it to 0..1.
- `filter` is a reserved word in WGSL, so a WGSL tool may complain about it. The app takes it as the
  name of your function and nothing else: it can't be used anywhere else in the file.

You're given three things:

| | |
|---|---|
| `v.<key>` | Each control's value, as an `f32`. A switch is 0 or 1 |
| `pixel(at)` | The layer as it was at `at`, straight colour. Past an edge, the edge's pixel |
| `canvas_size()` | The canvas's width and height, in pixels |

The app does the rest: premultiplying, the selection, the alpha lock and the copy of the layer. A
filter can't see the selection, and doesn't need to.

`craft-extension.json` describes it. `tools/pack_patch.py` lists the code in it for you.

```json
{
  "schema": 1,
  "id": "org.example.lines",
  "name": "Lines",
  "version": "1.0.0",
  "kind": "filter",
  "publisher": { "name": "Example" },
  "description": "One sentence on what it does.",
  "compat": { "procraft": { "interface": 1 } },
  "permissions": [],
  "controls": [
    { "key": "cut", "type": "slider", "label": "Cut-off", "least": 0, "most": 1, "step": 0.01,
      "starts": 0.5, "shown_times": 100, "unit": "%", "live": true },
    { "key": "keep_colour", "type": "switch", "label": "Keep the colour", "starts": false }
  ]
}
```

A control's `key` is its name in `v`, so it has to be a word WGSL allows as a field: lowercase
letters, digits and `_`, and not a keyword such as `loop`. `id` must have at least two dotted parts.

## Building it

```
python tools/pack_patch.py lines --out somewhere
```

That writes `lines.procraftplugin`. To try it, import it through the Plugins page.

## What is refused

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

## How long it may take

A GPU can't be interrupted, and a shader that runs too long can stop every app on the device. So a
pixel has a budget: about 65,000 simple operations in loops, where `sin` or `pow` counts as four and
`pixel()` as about twenty, and as much again outside them. A 31 by 31 neighbourhood fits. Every loop
pays for each round as it goes, and stops when the budget is spent.

Before a filter with a loop touches the layer, the app tries it on a few hundred pixels spread over
the layer. If one of them runs out, the filter is switched off, saying it took too long, and the
layer isn't touched. It stays off until someone taps Turn on. The same goes for a draw that takes
far longer than it can have, or a driver that fails on it.

A filter with no loop is drawn in one go. One with a loop is drawn in bands, so the screen keeps
going. If a whole draw takes more than about 0.15 seconds, sliders stop following the finger and
redraw as it lets go. If it takes more than 4 seconds, the filter is closed and the layer put back.

## When the app changes

`interface` in `compat` is the version of all this: the template around your function, what you're
given, and the kinds of control. The app keeps every interface's template, so a filter goes on working
as it was written. Something that needs more uses a newer interface number, and an older app says it
needs a newer ProCraft.
